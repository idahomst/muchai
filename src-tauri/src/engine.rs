use crate::command_builder::build_args;
use crate::progress_parser::{parse_image_seed_line, parse_progress_line};
use crate::types::{GenerationRequest, ProgressUpdate};
use std::io::{BufRead, BufReader};
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
}

/// Slot holding the running child so a separate `cancel` call can kill it.
pub type ChildSlot = Arc<Mutex<Option<Child>>>;

fn looks_like_oom(s: &str) -> bool {
    let l = s.to_lowercase();
    l.contains("out of memory") || l.contains("cuda error") || l.contains("oom")
}

/// Run one generation: spawn `binary` with args from `req`, stream stdout+stderr,
/// call `on_progress` for each parsed progress line, and map the exit status.
/// The engine writes the PNG to `output_path` itself. `slot` receives the child
/// handle so it can be cancelled.
/// On success, returns the actual seed of each generated image, ordered by the
/// engine's 1-based image index (so element 0 is the first image). May be empty
/// if the engine didn't announce seeds; callers then derive seeds themselves.
pub fn run_generation<F: FnMut(ProgressUpdate)>(
    binary: &Path,
    req: &GenerationRequest,
    output_path: &Path,
    slot: &ChildSlot,
    mut on_progress: F,
) -> Result<Vec<i64>, GenError> {
    if !binary.exists() {
        return Err(GenError::BinaryNotFound(binary.display().to_string()));
    }
    let args = build_args(req, &output_path.to_string_lossy());

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
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let h_err_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let h_err_lines2 = h_err_lines.clone();
    let h_err = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            h_err_lines2.lock().unwrap().push(line.clone());
            let _ = tx2.send(line);
        }
    });

    let mut seeds: Vec<(u32, i64)> = Vec::new();
    for line in rx {
        if let Some(update) = parse_progress_line(&line) {
            on_progress(update);
        } else if let Some(s) = parse_image_seed_line(&line) {
            seeds.push((s.index, s.seed));
        }
    }
    let _ = h_out.join();
    let _ = h_err.join();

    // Take the child back and wait for the exit status.
    // Slot is empty only if a cancel (Task 11) already took and killed the child;
    // that path owns wait()/reap, so we just report cancellation here.
    let mut child = slot.lock().unwrap().take().ok_or(GenError::NonZero {
        code: None,
        stderr_tail: "generation was cancelled".into(),
        oom: false,
    })?;
    let status = child.wait().map_err(|e| GenError::Spawn(e.to_string()))?;

    if status.success() {
        seeds.sort_by_key(|(i, _)| *i);
        Ok(seeds.into_iter().map(|(_, s)| s).collect())
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
        let dir = std::env::temp_dir().join(format!("fridai-eng-{}-{}", std::process::id(), name));
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
        let bin = write_fake_engine(
            "#!/bin/sh\necho '  |#####| 1/3'\necho '  |##########| 2/3'\necho '  |###############| 3/3'\nexit 0\n",
            "ok",
        );
        let slot: ChildSlot = Arc::new(Mutex::new(None));
        let updates = Arc::new(Mutex::new(Vec::new()));
        let u2 = updates.clone();
        let res = run_generation(
            &bin,
            &GenerationRequest::default(),
            Path::new("/tmp/ignored.png"),
            &slot,
            move |p| u2.lock().unwrap().push(p),
        );
        assert!(res.is_ok());
        let got = updates.lock().unwrap();
        assert_eq!(got.last().copied(), Some(ProgressUpdate { current_step: 3, total_steps: 3 }));
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
            &slot,
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
            &slot,
            |_| {},
        );
        assert!(matches!(res, Err(GenError::BinaryNotFound(_))));
    }
}
