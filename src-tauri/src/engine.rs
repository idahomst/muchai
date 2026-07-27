use crate::command_builder::{build_args, EngineOptions};
use crate::progress_parser::{
    parse_image_seed_line, parse_lora_warning_line, parse_progress_line, parse_resolved_seed_line,
};
use crate::types::{GenerationRequest, ProgressUpdate};
use std::io::{BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GenError {
    #[error("engine binary not found at {0}")]
    BinaryNotFound(String),
    #[error("failed to start engine: {0}")]
    Spawn(String),
    #[error("engine exited with code {code:?}{}", if *.oom { " (out of memory)" } else { "" })]
    NonZero { code: Option<i32>, stderr_tail: String, oom: bool },
    /// The run was killed by `cancel_generation` (user pressed Cancel). This is
    /// not a failure — callers treat it as a clean no-op, not an error.
    #[error("generation cancelled")]
    Cancelled,
}

/// Slot holding the running child so a separate `cancel` call can kill it.
pub type ChildSlot = Arc<Mutex<Option<Child>>>;

/// Read `r` and invoke `on_segment` for each chunk separated by '\n' OR '\r'.
///
/// stable-diffusion.cpp redraws its sampling bar in place with carriage returns
/// ("\r ...1/4... \r ...2/4... \r ...4/4... \n") and only emits a newline when
/// the phase finishes. `BufReader::lines()` splits on '\n' alone, so it would
/// collapse every per-step redraw into one line delivered at the very end,
/// making the step counter jump straight to the final value. Splitting on '\r'
/// too surfaces each step as it happens. Empty segments (e.g. a "\r\n" pair, or
/// a leading "\r") are dropped. Trailing ANSI erase codes like "\x1b[K" are left
/// in the segment; the progress/seed parsers ignore them.
fn read_segments<R: Read>(r: R, mut on_segment: impl FnMut(String)) {
    let mut reader = BufReader::new(r);
    let mut buf: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => match byte[0] {
                b'\n' | b'\r' => {
                    if !buf.is_empty() {
                        on_segment(String::from_utf8_lossy(&buf).into_owned());
                        buf.clear();
                    }
                }
                b => buf.push(b),
            },
            Err(_) => break,
        }
    }
    if !buf.is_empty() {
        on_segment(String::from_utf8_lossy(&buf).into_owned());
    }
}

fn looks_like_oom(s: &str) -> bool {
    let l = s.to_lowercase();
    l.contains("out of memory") || l.contains("cuda error") || l.contains("oom")
}

/// The backend selection and per-run engine options, bundled because the
/// caller always decides both together (see `commands::generate`) — and
/// because `run_generation` already sits at clippy's argument-count limit,
/// so a new parameter must come in bundled with an existing one rather than
/// added loose.
pub struct RunOptions<'a> {
    pub backend: Option<&'a str>,
    pub opts: EngineOptions,
}

/// Run one generation: spawn `binary` with args from `req`, stream stdout+stderr,
/// call `on_progress` for each parsed progress line, and map the exit status.
/// The engine writes the PNG to `output_path` itself. `slot` receives the child
/// handle so it can be cancelled.
/// On success, returns the actual seed of each generated image, ordered by the
/// engine's 1-based image index (so element 0 is the first image). May be empty
/// if the engine didn't announce seeds; callers then derive seeds themselves.
/// `on_notice` is called for each engine warning the user needs to see — today
/// only the missing-LoRA warning, which is otherwise invisible because the run
/// still succeeds.
pub fn run_generation<F: FnMut(ProgressUpdate), N: FnMut(String)>(
    binary: &Path,
    req: &GenerationRequest,
    output_path: &Path,
    run_opts: RunOptions,
    slot: &ChildSlot,
    mut on_progress: F,
    mut on_notice: N,
) -> Result<Vec<i64>, GenError> {
    if !binary.exists() {
        return Err(GenError::BinaryNotFound(binary.display().to_string()));
    }
    let args = build_args(req, &output_path.to_string_lossy(), run_opts.backend, run_opts.opts);

    let mut child = Command::new(binary)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| GenError::Spawn(e.to_string()))?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    // Store the child so `cancel` can kill it; we still own the pipe handles.
    *slot.lock().unwrap() = Some(child);

    // Merge both streams into one channel of lines.
    let (tx, rx) = mpsc::channel::<String>();
    let tx2 = tx.clone();
    let h_out = std::thread::spawn(move || {
        read_segments(stdout, |line| {
            let _ = tx.send(line);
        });
    });
    let h_err_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let h_err_lines2 = h_err_lines.clone();
    let h_err = std::thread::spawn(move || {
        read_segments(stderr, |line| {
            h_err_lines2.lock().unwrap().push(line.clone());
            let _ = tx2.send(line);
        });
    });

    let mut seeds: Vec<(u32, i64)> = Vec::new();
    let mut base_seed: Option<i64> = None;
    for line in rx {
        if let Some(update) = parse_progress_line(&line) {
            on_progress(update);
        } else if let Some(s) = parse_image_seed_line(&line) {
            seeds.push((s.index, s.seed));
        } else if let Some(b) = parse_resolved_seed_line(&line) {
            base_seed = Some(b);
        } else if let Some(name) = parse_lora_warning_line(&line) {
            on_notice(name);
        }
    }
    let _ = h_out.join();
    let _ = h_err.join();

    // Take the child back and wait for the exit status.
    // Slot is empty only if a cancel (Task 11) already took and killed the child;
    // that path owns wait()/reap, so we just report cancellation here. Cancel is
    // user-initiated, so it's a distinct outcome — not an engine failure.
    let mut child = slot.lock().unwrap().take().ok_or(GenError::Cancelled)?;
    let status = child.wait().map_err(|e| GenError::Spawn(e.to_string()))?;

    if status.success() {
        seeds.sort_by_key(|(i, _)| *i);
        let mut per_image: Vec<i64> = seeds.into_iter().map(|(_, s)| s).collect();
        // Fallback: some engine builds omit the per-image "generating image:
        // i/N - seed S" lines (notably for a single image), leaving us with
        // fewer seeds than images. When that happens but the engine echoed its
        // resolved base seed, derive each image's seed as base + index — sd.cpp
        // assigns batch seeds sequentially from the base. This keeps every run
        // reproducible, single or batch.
        let batch = req.batch_count.max(1) as usize;
        if per_image.len() < batch {
            if let Some(base) = base_seed {
                per_image = (0..batch).map(|i| base + i as i64).collect();
            }
        }
        Ok(per_image)
    } else {
        let tail = h_err_lines.lock().unwrap();
        let joined = tail.iter().rev().take(20).rev().cloned().collect::<Vec<_>>().join("\n");
        Err(GenError::NonZero {
            code: status.code(),
            oom: looks_like_oom(&joined),
            stderr_tail: joined,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    // Serialize fake-engine write+spawn across tests to avoid the
    // fork/exec ETXTBSY ("Text file busy") race: when tests run on parallel
    // threads, one test's concurrent fork can inherit the still-open writable
    // fd to another test's just-written sd-cli script, and the kernel refuses
    // to exec it. Holding this lock for each test body eliminates that overlap.
    fn spawn_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn write_fake_engine(script: &str, name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("muchai-eng-{}-{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sd-cli");
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn streams_progress_and_succeeds() {
        let _guard = spawn_lock().lock().unwrap();
        // Realistic sampling bars: each carries an iteration-rate suffix, which
        // is what the parser keys on to tell sampling from model-loading bars.
        let bin = write_fake_engine(
            "#!/bin/sh\nprintf '  |==>| 1/3 - 2.00s/it\\n'\nprintf '  |====>| 2/3 - 1.90s/it\\n'\nprintf '  |======>| 3/3 - 1.85it/s\\n'\nexit 0\n",
            "ok",
        );
        let slot: ChildSlot = Arc::new(Mutex::new(None));
        let updates = Arc::new(Mutex::new(Vec::new()));
        let u2 = updates.clone();
        let res = run_generation(
            &bin,
            &GenerationRequest::default(),
            Path::new("/tmp/ignored.png"),
            RunOptions { backend: None, opts: EngineOptions::default() },
            &slot,
            move |p| u2.lock().unwrap().push(p),
            |_| {},
        );
        assert!(res.is_ok());
        let got = updates.lock().unwrap();
        assert_eq!(
            *got,
            vec![
                ProgressUpdate { current_step: 1, total_steps: 3 },
                ProgressUpdate { current_step: 2, total_steps: 3 },
                ProgressUpdate { current_step: 3, total_steps: 3 },
            ]
        );
    }

    #[test]
    fn splits_carriage_return_redraws() {
        let _guard = spawn_lock().lock().unwrap();
        // The engine redraws the sampling bar in place with '\r' and only prints
        // a trailing '\n' when the phase finishes. All three steps arrive on ONE
        // physical line; read_segments must split on '\r' so each step surfaces.
        let bin = write_fake_engine(
            "#!/bin/sh\nprintf '  |=>| 1/3 - 2.00s/it\\r  |===>| 2/3 - 1.90s/it\\r  |=====>| 3/3 - 1.85it/s\\n'\nexit 0\n",
            "cr",
        );
        let slot: ChildSlot = Arc::new(Mutex::new(None));
        let updates = Arc::new(Mutex::new(Vec::new()));
        let u2 = updates.clone();
        let res = run_generation(
            &bin,
            &GenerationRequest::default(),
            Path::new("/tmp/ignored.png"),
            RunOptions { backend: None, opts: EngineOptions::default() },
            &slot,
            move |p| u2.lock().unwrap().push(p),
            |_| {},
        );
        assert!(res.is_ok());
        let got = updates.lock().unwrap();
        assert_eq!(
            *got,
            vec![
                ProgressUpdate { current_step: 1, total_steps: 3 },
                ProgressUpdate { current_step: 2, total_steps: 3 },
                ProgressUpdate { current_step: 3, total_steps: 3 },
            ]
        );
    }

    #[test]
    fn ignores_loading_bars() {
        let _guard = spawn_lock().lock().unwrap();
        // A model-loading bar (byte-rate suffix, tensor counts as N/M) must NOT
        // be reported as sampling progress; only the real sampling bar counts.
        let bin = write_fake_engine(
            "#!/bin/sh\nprintf '  |####| 686/686 - 2.02GB/s\\n'\nprintf '  |==>| 1/4 - 2.00s/it\\n'\nexit 0\n",
            "loading",
        );
        let slot: ChildSlot = Arc::new(Mutex::new(None));
        let updates = Arc::new(Mutex::new(Vec::new()));
        let u2 = updates.clone();
        let res = run_generation(
            &bin,
            &GenerationRequest::default(),
            Path::new("/tmp/ignored.png"),
            RunOptions { backend: None, opts: EngineOptions::default() },
            &slot,
            move |p| u2.lock().unwrap().push(p),
            |_| {},
        );
        assert!(res.is_ok());
        let got = updates.lock().unwrap();
        assert_eq!(*got, vec![ProgressUpdate { current_step: 1, total_steps: 4 }]);
    }

    #[test]
    fn derives_seed_from_base_when_per_image_line_absent() {
        // A single-image run where the engine only echoes its resolved base seed
        // (no "generating image: 1/1 - seed S" line) must still report that seed.
        let _guard = spawn_lock().lock().unwrap();
        let bin = write_fake_engine(
            "#!/bin/sh\necho '  seed: 1648302913,'\necho '  |#####| 1/1'\nexit 0\n",
            "baseseed",
        );
        let slot: ChildSlot = Arc::new(Mutex::new(None));
        let res = run_generation(
            &bin,
            &GenerationRequest::default(),
            Path::new("/tmp/ignored.png"),
            RunOptions { backend: None, opts: EngineOptions::default() },
            &slot,
            |_| {},
            |_| {},
        );
        assert_eq!(res.unwrap(), vec![1648302913]);
    }

    #[test]
    fn maps_oom_failure() {
        let _guard = spawn_lock().lock().unwrap();
        let bin = write_fake_engine(
            "#!/bin/sh\necho 'CUDA error: out of memory' 1>&2\nexit 2\n",
            "oom",
        );
        let slot: ChildSlot = Arc::new(Mutex::new(None));
        let res = run_generation(
            &bin,
            &GenerationRequest::default(),
            Path::new("/tmp/ignored.png"),
            RunOptions { backend: None, opts: EngineOptions::default() },
            &slot,
            |_| {},
            |_| {},
        );
        match res {
            Err(GenError::NonZero { oom, code, .. }) => {
                assert!(oom);
                assert_eq!(code, Some(2));
            }
            other => panic!("expected OOM NonZero, got {other:?}"),
        }
    }

    #[test]
    fn missing_binary_errors() {
        let _guard = spawn_lock().lock().unwrap();
        let slot: ChildSlot = Arc::new(Mutex::new(None));
        let res = run_generation(
            Path::new("/no/such/sd-cli"),
            &GenerationRequest::default(),
            Path::new("/tmp/ignored.png"),
            RunOptions { backend: None, opts: EngineOptions::default() },
            &slot,
            |_| {},
            |_| {},
        );
        assert!(matches!(res, Err(GenError::BinaryNotFound(_))));
    }

    #[test]
    fn reports_a_missing_lora_warning() {
        // The engine warns, then exits 0 with an unmodified image. The run must
        // still succeed AND the notice must reach the caller.
        let _guard = spawn_lock().lock().unwrap();
        let bin = write_fake_engine(
            "#!/bin/sh\necho \"[WARN ] can not found lora '/tmp/loras/film-grain.safetensors'\"\nexit 0\n",
            "loramissing",
        );
        let slot: ChildSlot = Arc::new(Mutex::new(None));
        let notices = Arc::new(Mutex::new(Vec::new()));
        let n2 = notices.clone();
        let res = run_generation(
            &bin,
            &GenerationRequest::default(),
            Path::new("/tmp/ignored.png"),
            RunOptions { backend: None, opts: EngineOptions::default() },
            &slot,
            |_| {},
            move |n| n2.lock().unwrap().push(n),
        );
        assert!(res.is_ok());
        assert_eq!(*notices.lock().unwrap(), vec!["film-grain".to_string()]);
    }
}
