# fridAI Beta (Phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a lightweight, self-contained desktop app that generates images from a text prompt by driving a bundled stable-diffusion.cpp binary, with live progress, a session history, and a GPU/VRAM/CPU/RAM monitor.

**Architecture:** Tauri app — a Rust backend exposes commands to a Svelte/TypeScript web UI (Layout A: left controls, center preview, bottom history strip, resource monitor). The Rust core spawns the `sd-cli` binary as a child process, streams its stdout/stderr to parse step progress, and saves each result as a PNG plus a JSON parameter sidecar. The image engine is shipped bundled (Tauri sidecar). No database — files only.

**Tech Stack:** Rust + Tauri v2, Svelte + TypeScript + Vite, stable-diffusion.cpp (`sd-cli`), crates: `serde`/`serde_json`, `uuid`, `directories`, `sysinfo`, `nvml-wrapper`, `thiserror`; Tauri `dialog` plugin.

**Conventions:**
- TDD for all pure Rust logic (`command_builder`, `progress_parser`, `config`, `gallery`, `sysmon`, `engine`). UI is verified manually (Svelte components hold minimal logic).
- All Rust unit tests live inline in `#[cfg(test)] mod tests` and run with `cargo test` from `src-tauri/`.
- Conventional commit messages. **Every commit message ends with the trailer** `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` (shown in each commit step via a second `-m`).
- Work happens at the repo root `/home/idaho/g/mst/fridai`. Rust crate lives in `src-tauri/`.

---

## File Structure

```
fridai/
  src/                              # Svelte frontend
    app.css                         # global theme
    main.ts                         # Svelte entry (scaffolded)
    App.svelte                      # Layout A assembly
    lib/
      api.ts                        # typed wrappers over Tauri invoke/listen
      stores.ts                     # Svelte stores: request, status, history, sysStats, settings
      types.ts                      # TS mirror of Rust DTOs
      components/
        ResourceMonitor.svelte
        ModelPicker.svelte
        SettingsPanel.svelte
        PromptPanel.svelte
        GenerateBar.svelte          # generate button + progress + error box
        ImagePreview.svelte
        HistoryStrip.svelte
  src-tauri/
    Cargo.toml
    tauri.conf.json
    capabilities/default.json
    fixtures/                       # captured engine interface (test data)
      sd-help.txt
      sd-sample-output.txt
    binaries/                       # bundled sidecar sd-cli (gitignored; see .gitignore /engine,/binaries)
    src/
      lib.rs                        # Tauri app setup, command registration, sysmon loop
      main.rs                       # thin entry calling lib::run()
      types.rs                      # GenerationRequest, Sampler, ProgressUpdate, GpuStats, SystemStats, GalleryItem, AppConfig
      command_builder.rs            # GenerationRequest -> Vec<String> CLI args (pure)
      progress_parser.rs            # output line -> Option<ProgressUpdate> (pure)
      config.rs                     # load/save AppConfig, path resolution
      gallery.rs                    # write sidecar JSON, list items
      sysmon.rs                     # NVML + sysinfo -> SystemStats
      engine.rs                     # run_generation: spawn sd-cli, stream, map errors, cancel
      commands.rs                   # #[tauri::command] surface
```

Each Rust module has one responsibility and pure-logic modules (`command_builder`, `progress_parser`, `config`, `gallery`) do no process spawning, so they unit-test without a GPU. `engine.rs` is tested against a fake shell-script "engine".

---

## Task 1: Scaffold the Tauri + Svelte app

**Files:**
- Create (via scaffolder, then moved into repo root): `package.json`, `vite.config.ts`, `svelte.config.js`, `tsconfig.json`, `index.html`, `src/main.ts`, `src/App.svelte`, `src-tauri/` (Cargo.toml, tauri.conf.json, src/main.rs, src/lib.rs, capabilities/default.json)
- Modify: `.gitignore` (merge scaffolder entries if any are new)

- [ ] **Step 1: Scaffold into a temp dir** (create-tauri-app refuses a non-empty dir, so we scaffold then move)

```bash
rm -rf /tmp/fridai-scaffold
npm create tauri-app@latest /tmp/fridai-scaffold -- \
  --manager npm --template svelte-ts --identifier cz.mst.fridai --yes
```
Expected: a new project under `/tmp/fridai-scaffold` containing `src/`, `src-tauri/`, `package.json`, etc.

- [ ] **Step 2: Move scaffold contents into the repo root** (keep our `docs/`, `.git`, `.gitignore`, `.superpowers/`)

```bash
cd /home/idaho/g/mst/fridai
# copy everything except git metadata
rsync -a --exclude='.git' --exclude='.gitignore' /tmp/fridai-scaffold/ ./
# if the scaffold shipped its own .gitignore, append any unique lines to ours
[ -f /tmp/fridai-scaffold/.gitignore ] && cat /tmp/fridai-scaffold/.gitignore >> .gitignore.scaffold && sort -u .gitignore .gitignore.scaffold -o .gitignore && rm -f .gitignore.scaffold
```

- [ ] **Step 3: Install JS deps**

Run: `npm install`
Expected: `node_modules/` created, no errors.

- [ ] **Step 4: Verify the app builds and the dev window opens**

Run: `npm run tauri dev`
Expected: a native window opens showing the default Tauri+Svelte starter page. Close it (Ctrl+C in terminal).

> If `npm run tauri dev` fails for missing system libs, install the Tauri Linux prerequisites (`webkit2gtk-4.1`, `libappindicator`, `librsvg`, `build-essential`, etc.) per the Tauri docs, then retry. Do not proceed until the window opens.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: scaffold Tauri v2 + Svelte TypeScript app" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Add Rust dependencies and the dialog plugin

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Add crates to `src-tauri/Cargo.toml`** under `[dependencies]` (keep the existing `tauri`, `serde`, `serde_json` lines that the scaffold added; add the rest)

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
directories = "5"
sysinfo = "0.33"
nvml-wrapper = "0.10"
thiserror = "2"
tauri-plugin-dialog = "2"
```

- [ ] **Step 2: Register the dialog plugin** (Tauri v2 CLI wires Rust + JS + permissions)

Run: `npm run tauri add dialog`
Expected: command reports it added `tauri-plugin-dialog` and updated capabilities.

- [ ] **Step 3: Verify the crate still compiles**

Run: `cd src-tauri && cargo build && cd ..`
Expected: builds successfully (may take a while on first compile).

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: add backend deps (sysinfo, nvml, directories, uuid) and dialog plugin" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Obtain `sd-cli` and capture its interface as fixtures

This pins the real command-line interface and output format so the parser/builder tasks are tested against ground truth instead of guesses.

**Files:**
- Create: `src-tauri/fixtures/sd-help.txt`
- Create: `src-tauri/fixtures/sd-sample-output.txt`

- [ ] **Step 1: Get a CUDA build of `sd-cli`**

Download a prebuilt CUDA release of stable-diffusion.cpp from its GitHub Releases (asset containing `sd-cli` built with CUDA), or build it. Place the binary at `/home/idaho/g/mst/fridai/src-tauri/binaries/sd-cli` and mark it executable:

```bash
chmod +x src-tauri/binaries/sd-cli
src-tauri/binaries/sd-cli --help | head -5
```
Expected: prints a version/usage banner without crashing.

- [ ] **Step 2: Capture the help text as a fixture**

```bash
src-tauri/binaries/sd-cli --help > src-tauri/fixtures/sd-help.txt 2>&1
```
Expected: `sd-help.txt` lists the real flags. **Read it** and note the exact spellings for: model, prompt, negative prompt, steps, cfg scale, sampling method (and valid sampler names), width, height, seed, batch count, output, verbose, and the mode flag (`-M`/`--mode`) if present. These confirm Task 5.

- [ ] **Step 3: Capture a real generation's output as a fixture** (use any local model you already have)

```bash
src-tauri/binaries/sd-cli -m /path/to/your/model.safetensors \
  -p "a lovely cat" --steps 6 -W 256 -H 256 -o /tmp/fridai-cap.png -v \
  > src-tauri/fixtures/sd-sample-output.txt 2>&1
```
Expected: an image at `/tmp/fridai-cap.png` and `sd-sample-output.txt` containing the real progress lines. **Read it** and note the exact progress-bar line format (e.g. whether it shows `5/6`, which stream it's on, the `\r` carriage returns). This is the ground truth for Task 6.

- [ ] **Step 4: Commit the fixtures** (the binary itself is gitignored via `/binaries/` — only text fixtures are committed)

```bash
git add src-tauri/fixtures/
git commit -m "test: capture sd-cli help and sample output as fixtures" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

> Add `/src-tauri/binaries/` to `.gitignore` if not already covered, so the engine binary is never committed.

---

## Task 4: Shared types

**Files:**
- Create: `src-tauri/src/types.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod types;`)

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/types.rs` with the types and a serde round-trip test:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sampler {
    Euler,
    EulerA,
    Heun,
    Dpm2,
    DpmPp2sA,
    DpmPp2m,
    DpmPp2mV2,
    Ipndm,
    IpndmV,
    Lcm,
}

impl Sampler {
    /// Exact token stable-diffusion.cpp expects after its sampling-method flag.
    /// Reconcile these against `fixtures/sd-help.txt` in Task 5.
    pub fn cli_name(self) -> &'static str {
        match self {
            Sampler::Euler => "euler",
            Sampler::EulerA => "euler_a",
            Sampler::Heun => "heun",
            Sampler::Dpm2 => "dpm2",
            Sampler::DpmPp2sA => "dpm++2s_a",
            Sampler::DpmPp2m => "dpm++2m",
            Sampler::DpmPp2mV2 => "dpm++2mv2",
            Sampler::Ipndm => "ipndm",
            Sampler::IpndmV => "ipndm_v",
            Sampler::Lcm => "lcm",
        }
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Sampler::EulerA
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationRequest {
    pub model_path: String,
    pub prompt: String,
    pub negative_prompt: String,
    pub steps: u32,
    pub cfg_scale: f32,
    pub sampler: Sampler,
    pub width: u32,
    pub height: u32,
    pub seed: i64, // -1 = random
    pub batch_count: u32,
}

impl Default for GenerationRequest {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            prompt: String::new(),
            negative_prompt: String::new(),
            steps: 20,
            cfg_scale: 7.0,
            sampler: Sampler::default(),
            width: 512,
            height: 512,
            seed: -1,
            batch_count: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub current_step: u32,
    pub total_steps: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuStats {
    pub name: String,
    pub utilization_pct: u32,
    pub vram_used_mb: u64,
    pub vram_total_mb: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemStats {
    pub gpu: Option<GpuStats>,
    pub cpu_pct: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GalleryItem {
    pub id: String,
    pub image_path: String,
    pub request: GenerationRequest,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub sd_binary_path: Option<String>, // None => use bundled sidecar
    pub default_model_path: Option<String>,
    pub gallery_dir: String,
    pub last_request: GenerationRequest,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_request_round_trips_through_json() {
        let req = GenerationRequest {
            prompt: "a lovely cat".into(),
            sampler: Sampler::DpmPp2m,
            seed: 1234,
            ..Default::default()
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: GenerationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn sampler_serializes_snake_case() {
        let json = serde_json::to_string(&Sampler::EulerA).unwrap();
        assert_eq!(json, "\"euler_a\"");
    }
}
```

- [ ] **Step 2: Register the module** — add to the top of `src-tauri/src/lib.rs`:

```rust
mod types;
```

- [ ] **Step 3: Run the tests**

Run: `cd src-tauri && cargo test types:: && cd ..`
Expected: both tests PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add shared backend types (request, progress, stats, config)" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: command_builder (TDD)

**Files:**
- Create: `src-tauri/src/command_builder.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod command_builder;`)

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/command_builder.rs`:

```rust
use crate::types::GenerationRequest;

/// Build the argument vector for stable-diffusion.cpp's CLI.
/// Pure function (no I/O) so it is fully unit-testable.
/// Flag spellings are confirmed against `fixtures/sd-help.txt` (Task 3).
pub fn build_args(req: &GenerationRequest, output_path: &str) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    let mut push = |flag: &str, val: String| {
        a.push(flag.to_string());
        a.push(val);
    };
    a.push("-M".into());
    a.push("txt2img".into());
    push("-m", req.model_path.clone());
    push("-p", req.prompt.clone());
    if !req.negative_prompt.is_empty() {
        push("-n", req.negative_prompt.clone());
    }
    push("--steps", req.steps.to_string());
    push("--cfg-scale", format!("{}", req.cfg_scale));
    push("--sampling-method", req.sampler.cli_name().to_string());
    push("-W", req.width.to_string());
    push("-H", req.height.to_string());
    push("-s", req.seed.to_string());
    push("-b", req.batch_count.to_string());
    push("-o", output_path.to_string());
    a.push("-v".into()); // verbose: ensures progress lines are emitted
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Sampler;

    fn sample() -> GenerationRequest {
        GenerationRequest {
            model_path: "/m/model.safetensors".into(),
            prompt: "a cat".into(),
            negative_prompt: "blurry".into(),
            steps: 25,
            cfg_scale: 7.5,
            sampler: Sampler::DpmPp2m,
            width: 768,
            height: 512,
            seed: 42,
            batch_count: 2,
        }
    }

    /// Helper: value immediately following a flag.
    fn val_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.iter().position(|x| x == flag).map(|i| args[i + 1].as_str())
    }

    #[test]
    fn includes_core_flags_and_values() {
        let args = build_args(&sample(), "/out/x.png");
        assert_eq!(val_after(&args, "-m"), Some("/m/model.safetensors"));
        assert_eq!(val_after(&args, "-p"), Some("a cat"));
        assert_eq!(val_after(&args, "-n"), Some("blurry"));
        assert_eq!(val_after(&args, "--steps"), Some("25"));
        assert_eq!(val_after(&args, "--cfg-scale"), Some("7.5"));
        assert_eq!(val_after(&args, "--sampling-method"), Some("dpm++2m"));
        assert_eq!(val_after(&args, "-W"), Some("768"));
        assert_eq!(val_after(&args, "-H"), Some("512"));
        assert_eq!(val_after(&args, "-s"), Some("42"));
        assert_eq!(val_after(&args, "-b"), Some("2"));
        assert_eq!(val_after(&args, "-o"), Some("/out/x.png"));
    }

    #[test]
    fn omits_negative_prompt_when_empty() {
        let mut req = sample();
        req.negative_prompt = "".into();
        let args = build_args(&req, "/out/x.png");
        assert!(!args.iter().any(|x| x == "-n"));
    }
}
```

- [ ] **Step 2: Register the module** — add to `src-tauri/src/lib.rs`:

```rust
mod command_builder;
```

- [ ] **Step 3: Run the tests**

Run: `cd src-tauri && cargo test command_builder:: && cd ..`
Expected: both tests PASS.

- [ ] **Step 4: Reconcile flags against the captured help** — open `src-tauri/fixtures/sd-help.txt`. For any flag whose real spelling differs (e.g. the mode flag is `--mode` not `-M`, or sampler tokens differ), edit `build_args` and `Sampler::cli_name` to match the fixture, then re-run `cargo test command_builder::`. Expected: still PASS (update the asserted strings if you changed a sampler token).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: build sd-cli argument vector from a generation request" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: progress_parser (TDD)

**Files:**
- Create: `src-tauri/src/progress_parser.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod progress_parser;`)

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/progress_parser.rs`:

```rust
use crate::types::ProgressUpdate;

/// Parse one line of engine output into a progress update, if present.
/// stable-diffusion.cpp prints a sampling bar like:
///   "  |==========>            | 5/30 - 2.34it/s"
/// We only treat a line as progress if it contains a '|' bar, then read the
/// LAST "<digits>/<digits>" pair on the line. Returns None otherwise.
pub fn parse_progress_line(line: &str) -> Option<ProgressUpdate> {
    if !line.contains('|') {
        return None;
    }
    let bytes = line.as_bytes();
    let mut best: Option<(u32, u32)> = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'/' {
                let slash = i;
                i += 1;
                let tstart = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i > tstart {
                    if let (Ok(c), Ok(t)) =
                        (line[start..slash].parse::<u32>(), line[tstart..i].parse::<u32>())
                    {
                        if t > 0 {
                            best = Some((c, t));
                        }
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    best.map(|(current_step, total_steps)| ProgressUpdate { current_step, total_steps })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_progress_bar_line() {
        let line = "  |==========>            | 5/30 - 2.34it/s";
        assert_eq!(
            parse_progress_line(line),
            Some(ProgressUpdate { current_step: 5, total_steps: 30 })
        );
    }

    #[test]
    fn ignores_non_bar_lines() {
        assert_eq!(parse_progress_line("[INFO] loading model 1/1"), None);
        assert_eq!(parse_progress_line("done"), None);
    }

    #[test]
    fn takes_last_pair_on_the_line() {
        // a bar that also mentions another ratio earlier
        let line = "batch 1/2 |######| 30/30 - 1.0it/s";
        assert_eq!(
            parse_progress_line(line),
            Some(ProgressUpdate { current_step: 30, total_steps: 30 })
        );
    }
}
```

- [ ] **Step 2: Register the module** — add to `src-tauri/src/lib.rs`:

```rust
mod progress_parser;
```

- [ ] **Step 3: Run the tests**

Run: `cd src-tauri && cargo test progress_parser:: && cd ..`
Expected: all three tests PASS.

- [ ] **Step 4: Add a fixture-driven test** — append inside the `tests` module, using a real captured line. Open `src-tauri/fixtures/sd-sample-output.txt`, copy one real progress line verbatim, and assert it parses to the right total (replace `<TOTAL>` with the steps you used in Task 3, e.g. 6):

```rust
    #[test]
    fn parses_a_real_captured_line() {
        let captured = include_str!("../fixtures/sd-sample-output.txt");
        // At least one line must parse, and the max total must equal the run's step count.
        let max_total = captured
            .lines()
            .filter_map(parse_progress_line)
            .map(|p| p.total_steps)
            .max();
        assert_eq!(max_total, Some(6)); // <-- the --steps value used in Task 3
    }
```

If the real format doesn't match the parser (e.g. no `|` bar, different separators), adjust `parse_progress_line` until this passes. Run: `cd src-tauri && cargo test progress_parser:: && cd ..` → Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: parse sd-cli sampling progress from output lines" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: config (TDD)

**Files:**
- Create: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod config;`)

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/config.rs`:

```rust
use crate::types::{AppConfig, GenerationRequest};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("cz", "mst", "fridai")
}

pub fn default_gallery_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().join("gallery"))
        .unwrap_or_else(|| PathBuf::from("./gallery"))
}

pub fn config_file_path() -> PathBuf {
    project_dirs()
        .map(|d| d.config_dir().join("config.json"))
        .unwrap_or_else(|| PathBuf::from("./fridai-config.json"))
}

pub fn default_config() -> AppConfig {
    AppConfig {
        sd_binary_path: None,
        default_model_path: None,
        gallery_dir: default_gallery_dir().to_string_lossy().into_owned(),
        last_request: GenerationRequest::default(),
    }
}

/// Load config from a path; on missing file or parse error, return defaults.
pub fn load_config_from(path: &Path) -> AppConfig {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| default_config()),
        Err(_) => default_config(),
    }
}

/// Save config to a path, creating parent directories as needed.
pub fn save_config_to(path: &Path, cfg: &AppConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(cfg).expect("config serializes");
    std::fs::write(path, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let cfg = load_config_from(Path::new("/nonexistent/fridai/none.json"));
        assert!(cfg.sd_binary_path.is_none());
        assert_eq!(cfg.last_request.steps, 20);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut cfg = default_config();
        cfg.default_model_path = Some("/m/x.safetensors".into());
        cfg.last_request.prompt = "hello".into();
        save_config_to(&path, &cfg).unwrap();
        let back = load_config_from(&path);
        assert_eq!(back, cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: Register the module** — add to `src-tauri/src/lib.rs`:

```rust
mod config;
```

- [ ] **Step 3: Run the tests**

Run: `cd src-tauri && cargo test config:: && cd ..`
Expected: both tests PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: load and save app config with sensible defaults" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: gallery (TDD)

**Files:**
- Create: `src-tauri/src/gallery.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod gallery;`)

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/gallery.rs`:

```rust
use crate::types::GalleryItem;
use std::path::{Path, PathBuf};

/// Write the parameter sidecar JSON next to an image (same stem, ".json").
pub fn write_sidecar(image_path: &Path, item: &GalleryItem) -> std::io::Result<PathBuf> {
    let sidecar = image_path.with_extension("json");
    let s = serde_json::to_string_pretty(item).expect("gallery item serializes");
    std::fs::write(&sidecar, s)?;
    Ok(sidecar)
}

/// List gallery items in a directory by reading every "*.json" sidecar,
/// newest first.
pub fn list_items(dir: &Path) -> Vec<GalleryItem> {
    let mut items: Vec<GalleryItem> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(s) = std::fs::read_to_string(&path) {
                    if let Ok(item) = serde_json::from_str::<GalleryItem>(&s) {
                        items.push(item);
                    }
                }
            }
        }
    }
    items.sort_by(|a, b| b.created_at_unix.cmp(&a.created_at_unix));
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GenerationRequest;

    fn item(id: &str, ts: u64) -> GalleryItem {
        GalleryItem {
            id: id.into(),
            image_path: format!("/g/{id}.png"),
            request: GenerationRequest::default(),
            created_at_unix: ts,
        }
    }

    #[test]
    fn writes_sidecar_and_lists_newest_first() {
        let dir = std::env::temp_dir().join(format!("fridai-gal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        write_sidecar(&dir.join("older.png"), &item("older", 100)).unwrap();
        write_sidecar(&dir.join("newer.png"), &item("newer", 200)).unwrap();

        let listed = list_items(&dir);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "newer"); // newest first
        assert_eq!(listed[1].id, "older");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: Register the module** — add to `src-tauri/src/lib.rs`:

```rust
mod gallery;
```

- [ ] **Step 3: Run the tests**

Run: `cd src-tauri && cargo test gallery:: && cd ..`
Expected: the test PASSES.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: write and list gallery parameter sidecars" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: sysmon (TDD where possible)

**Files:**
- Create: `src-tauri/src/sysmon.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod sysmon;`)

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/sysmon.rs`:

```rust
use crate::types::{GpuStats, SystemStats};
use nvml_wrapper::Nvml;
use sysinfo::System;

/// Gather CPU/RAM via sysinfo and (optionally) GPU via NVML.
/// `nvml` = None hides the GPU section (e.g. on non-NVIDIA machines).
pub fn gather(sys: &mut System, nvml: Option<&Nvml>) -> SystemStats {
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    let cpu_pct = sys.global_cpu_usage();
    let ram_used_mb = sys.used_memory() / 1024 / 1024; // sysinfo 0.30+ returns bytes
    let ram_total_mb = sys.total_memory() / 1024 / 1024;
    let gpu = nvml.and_then(gather_gpu);
    SystemStats { gpu, cpu_pct, ram_used_mb, ram_total_mb }
}

fn gather_gpu(nvml: &Nvml) -> Option<GpuStats> {
    let device = nvml.device_by_index(0).ok()?;
    let name = device.name().ok()?;
    let util = device.utilization_rates().ok()?;
    let mem = device.memory_info().ok()?;
    Some(GpuStats {
        name,
        utilization_pct: util.gpu,
        vram_used_mb: mem.used / 1024 / 1024,
        vram_total_mb: mem.total / 1024 / 1024,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gathers_cpu_and_ram_without_gpu() {
        let mut sys = System::new();
        let stats = gather(&mut sys, None);
        assert!(stats.gpu.is_none());
        assert!(stats.ram_total_mb > 0);
        assert!(stats.ram_used_mb <= stats.ram_total_mb);
    }
}
```

- [ ] **Step 2: Register the module** — add to `src-tauri/src/lib.rs`:

```rust
mod sysmon;
```

- [ ] **Step 3: Run the tests**

Run: `cd src-tauri && cargo test sysmon:: && cd ..`
Expected: the test PASSES. (If `sysinfo` API names differ in the resolved version, adjust `global_cpu_usage`/`used_memory` to the version's equivalents until it compiles and passes.)

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: gather GPU/VRAM/CPU/RAM system stats" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 10: engine — spawn, stream, map errors, cancel (TDD with a fake binary)

**Files:**
- Create: `src-tauri/src/engine.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod engine;`)

- [ ] **Step 1: Write the failing test + implementation** — create `src-tauri/src/engine.rs`:

```rust
use crate::command_builder::build_args;
use crate::progress_parser::parse_progress_line;
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
pub fn run_generation<F: FnMut(ProgressUpdate)>(
    binary: &Path,
    req: &GenerationRequest,
    output_path: &Path,
    slot: &ChildSlot,
    mut on_progress: F,
) -> Result<(), GenError> {
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

    for line in rx {
        if let Some(update) = parse_progress_line(&line) {
            on_progress(update);
        }
    }
    let _ = h_out.join();
    let _ = h_err.join();

    // Take the child back and wait for the exit status.
    let mut child = slot.lock().unwrap().take().ok_or(GenError::NonZero {
        code: None,
        stderr_tail: "generation was cancelled".into(),
        oom: false,
    })?;
    let status = child.wait().map_err(|e| GenError::Spawn(e.to_string()))?;

    if status.success() {
        Ok(())
    } else {
        let tail = h_err_lines.lock().unwrap();
        let joined = tail.iter().rev().take(20).cloned().collect::<Vec<_>>().join("\n");
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
```

- [ ] **Step 2: Register the module** — add to `src-tauri/src/lib.rs`:

```rust
mod engine;
```

- [ ] **Step 3: Run the tests**

Run: `cd src-tauri && cargo test engine:: && cd ..`
Expected: all three tests PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: run sd-cli generation with streamed progress and error mapping" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: Tauri command surface + app state + sysmon loop

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (state, module wiring, command registration, background stats loop)

- [ ] **Step 1: Write `src-tauri/src/commands.rs`**

```rust
use crate::engine::{self, ChildSlot, GenError};
use crate::types::{AppConfig, GalleryItem, GenerationRequest};
use crate::{config, gallery};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub child: ChildSlot,
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Resolve the engine binary: explicit config path, else the bundled sidecar
/// next to the running executable.
fn resolve_binary(cfg: &AppConfig) -> Option<PathBuf> {
    if let Some(p) = &cfg.sd_binary_path {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) { "sd-cli.exe" } else { "sd-cli" };
    let cand = dir.join(name);
    cand.exists().then_some(cand)
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> AppConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_settings(state: State<AppState>, config: AppConfig) -> Result<(), String> {
    config::save_config_to(&config::config_file_path(), &config).map_err(|e| e.to_string())?;
    *state.config.lock().unwrap() = config;
    Ok(())
}

#[tauri::command]
pub fn list_history(state: State<AppState>) -> Vec<GalleryItem> {
    let dir = state.config.lock().unwrap().gallery_dir.clone();
    gallery::list_items(std::path::Path::new(&dir))
}

#[tauri::command]
pub fn cancel_generation(state: State<AppState>) {
    if let Some(mut child) = state.child.lock().unwrap().take() {
        let _ = child.kill();
    }
}

#[tauri::command]
pub async fn generate(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GenerationRequest,
) -> Result<GalleryItem, String> {
    let cfg = state.config.lock().unwrap().clone();
    let binary = resolve_binary(&cfg)
        .ok_or_else(|| "stable-diffusion engine not found. Set its path in Settings.".to_string())?;

    let gallery_dir = PathBuf::from(&cfg.gallery_dir);
    std::fs::create_dir_all(&gallery_dir).map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let image_path = gallery_dir.join(format!("{id}.png"));

    let slot = state.child.clone();
    let app2 = app.clone();
    let req = request.clone();
    let img = image_path.clone();

    // Run the (blocking) engine on a worker thread so the async command yields.
    let result = tauri::async_runtime::spawn_blocking(move || {
        engine::run_generation(&binary, &req, &img, &slot, |p| {
            let _ = app2.emit("generation:progress", p);
        })
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(()) => {
            let item = GalleryItem {
                id,
                image_path: image_path.to_string_lossy().into_owned(),
                request,
                created_at_unix: now_unix(),
            };
            gallery::write_sidecar(&image_path, &item).map_err(|e| e.to_string())?;
            // persist last-used request
            {
                let mut c = state.config.lock().unwrap();
                c.last_request = item.request.clone();
                let _ = config::save_config_to(&config::config_file_path(), &c);
            }
            Ok(item)
        }
        Err(GenError::NonZero { oom: true, .. }) => Err(
            "Out of GPU memory. Try a smaller width/height or batch count.".to_string(),
        ),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn pick_model_file(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let file = app
        .dialog()
        .file()
        .add_filter("Models", &["safetensors", "gguf", "ckpt"])
        .blocking_pick_file();
    file.and_then(|f| f.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}

// Re-export for the asset path conversion done in the frontend.
pub fn _unused(_: &Manager<tauri::Wry>) {}
```

- [ ] **Step 2: Wire `src-tauri/src/lib.rs`** — replace the scaffolded `run()` with state, modules, the stats loop, and command registration. The file should read:

```rust
mod command_builder;
mod commands;
mod config;
mod engine;
mod gallery;
mod progress_parser;
mod sysmon;
mod types;

use commands::AppState;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial = config::load_config_from(&config::config_file_path());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            config: Mutex::new(initial),
            child: Arc::new(Mutex::new(None)),
        })
        .setup(|app| {
            // Background system-stats loop: emit "system:stats" ~every second.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let nvml = nvml_wrapper::Nvml::init().ok();
                let mut sys = sysinfo::System::new();
                loop {
                    let stats = sysmon::gather(&mut sys, nvml.as_ref());
                    let _ = handle.emit("system:stats", stats);
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::list_history,
            commands::generate,
            commands::cancel_generation,
            commands::pick_model_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running fridAI");
}
```

- [ ] **Step 3: Confirm the whole crate compiles and all tests still pass**

Run: `cd src-tauri && cargo build && cargo test && cd ..`
Expected: build succeeds; all unit tests from Tasks 4–10 PASS.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: expose Tauri commands and emit system stats" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 12: Frontend types + typed API wrappers

**Files:**
- Create: `src/lib/types.ts`
- Create: `src/lib/api.ts`

- [ ] **Step 1: Create `src/lib/types.ts`** (mirror of the Rust DTOs)

```ts
export type Sampler =
  | "euler" | "euler_a" | "heun" | "dpm2"
  | "dpm++2s_a" | "dpm++2m" | "dpm++2mv2"
  | "ipndm" | "ipndm_v" | "lcm";

export interface GenerationRequest {
  model_path: string;
  prompt: string;
  negative_prompt: string;
  steps: number;
  cfg_scale: number;
  sampler: Sampler;
  width: number;
  height: number;
  seed: number;       // -1 = random
  batch_count: number;
}

export interface ProgressUpdate { current_step: number; total_steps: number; }

export interface GpuStats {
  name: string; utilization_pct: number;
  vram_used_mb: number; vram_total_mb: number;
}
export interface SystemStats {
  gpu: GpuStats | null; cpu_pct: number;
  ram_used_mb: number; ram_total_mb: number;
}

export interface GalleryItem {
  id: string; image_path: string;
  request: GenerationRequest; created_at_unix: number;
}

export interface AppConfig {
  sd_binary_path: string | null;
  default_model_path: string | null;
  gallery_dir: string;
  last_request: GenerationRequest;
}

export const defaultRequest = (): GenerationRequest => ({
  model_path: "", prompt: "", negative_prompt: "",
  steps: 20, cfg_scale: 7.0, sampler: "euler_a",
  width: 512, height: 512, seed: -1, batch_count: 1,
});

export const SAMPLERS: Sampler[] = [
  "euler_a", "euler", "heun", "dpm2",
  "dpm++2s_a", "dpm++2m", "dpm++2mv2", "ipndm", "ipndm_v", "lcm",
];
```

- [ ] **Step 2: Create `src/lib/api.ts`** (wrap `invoke`/`listen`/`convertFileSrc`)

```ts
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, GalleryItem, GenerationRequest, ProgressUpdate, SystemStats } from "./types";

export const getSettings = () => invoke<AppConfig>("get_settings");
export const setSettings = (config: AppConfig) => invoke<void>("set_settings", { config });
export const listHistory = () => invoke<GalleryItem[]>("list_history");
export const generate = (request: GenerationRequest) => invoke<GalleryItem>("generate", { request });
export const cancelGeneration = () => invoke<void>("cancel_generation");
export const pickModelFile = () => invoke<string | null>("pick_model_file");

export const imageSrc = (path: string) => convertFileSrc(path);

export const onProgress = (cb: (p: ProgressUpdate) => void): Promise<UnlistenFn> =>
  listen<ProgressUpdate>("generation:progress", (e) => cb(e.payload));

export const onSystemStats = (cb: (s: SystemStats) => void): Promise<UnlistenFn> =>
  listen<SystemStats>("system:stats", (e) => cb(e.payload));
```

- [ ] **Step 3: Enable the asset protocol for the gallery dir** — in `src-tauri/tauri.conf.json`, under `app.security`, add (create the keys if absent):

```json
"assetProtocol": { "enable": true, "scope": ["$APPDATA/**", "$HOME/.local/share/fridai/**"] }
```

And in `src-tauri/capabilities/default.json`, ensure the `permissions` array includes `"core:default"` and `"dialog:default"` (the latter added by Task 2).

- [ ] **Step 4: Verify the frontend type-checks**

Run: `npm run check` (svelte-check; if the script is absent, run `npx svelte-check`)
Expected: 0 errors.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: frontend DTOs and typed Tauri API wrappers" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 13: Frontend stores

**Files:**
- Create: `src/lib/stores.ts`

- [ ] **Step 1: Create `src/lib/stores.ts`**

```ts
import { writable } from "svelte/store";
import type { GenerationRequest, GalleryItem, SystemStats, AppConfig, ProgressUpdate } from "./types";
import { defaultRequest } from "./types";

export const request = writable<GenerationRequest>(defaultRequest());
export const settings = writable<AppConfig | null>(null);
export const history = writable<GalleryItem[]>([]);
export const currentImage = writable<string | null>(null); // converted asset src
export const sysStats = writable<SystemStats | null>(null);

export type GenStatus =
  | { kind: "idle" }
  | { kind: "running"; progress: ProgressUpdate | null }
  | { kind: "error"; message: string };

export const genStatus = writable<GenStatus>({ kind: "idle" });
```

- [ ] **Step 2: Commit**

```bash
git add -A
git commit -m "feat: frontend Svelte stores for request, status, history, stats" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 14: Components — ResourceMonitor, SettingsPanel, PromptPanel, ModelPicker

**Files:**
- Create: `src/lib/components/ResourceMonitor.svelte`
- Create: `src/lib/components/ModelPicker.svelte`
- Create: `src/lib/components/PromptPanel.svelte`
- Create: `src/lib/components/SettingsPanel.svelte`

- [ ] **Step 1: `ResourceMonitor.svelte`** (compact GPU/VRAM/CPU/RAM readout)

```svelte
<script lang="ts">
  import { sysStats } from "../stores";
  const mb = (v: number) => `${(v / 1024).toFixed(1)} GB`;
</script>

<div class="monitor">
  {#if $sysStats}
    {#if $sysStats.gpu}
      <span title="GPU">{$sysStats.gpu.name} · {$sysStats.gpu.utilization_pct}%</span>
      <span title="VRAM">VRAM {mb($sysStats.gpu.vram_used_mb)}/{mb($sysStats.gpu.vram_total_mb)}</span>
    {:else}
      <span>No NVIDIA GPU detected</span>
    {/if}
    <span title="CPU">CPU {$sysStats.cpu_pct.toFixed(0)}%</span>
    <span title="RAM">RAM {mb($sysStats.ram_used_mb)}/{mb($sysStats.ram_total_mb)}</span>
  {:else}
    <span>reading system…</span>
  {/if}
</div>

<style>
  .monitor { display:flex; gap:1rem; font-size:.75rem; opacity:.8; padding:.4rem .6rem;
    border-top:1px solid var(--border); white-space:nowrap; overflow-x:auto; }
</style>
```

- [ ] **Step 2: `ModelPicker.svelte`**

```svelte
<script lang="ts">
  import { request } from "../stores";
  import { pickModelFile } from "../api";
  async function pick() {
    const p = await pickModelFile();
    if (p) request.update((r) => ({ ...r, model_path: p }));
  }
  const basename = (p: string) => p.split("/").pop() ?? p;
</script>

<div class="field">
  <label class="label">Model</label>
  <div class="row">
    <button class="btn-secondary" on:click={pick}>Choose…</button>
    <span class="path" title={$request.model_path}>
      {$request.model_path ? basename($request.model_path) : "no model selected"}
    </span>
  </div>
</div>

<style>
  .row { display:flex; gap:.5rem; align-items:center; }
  .path { font-size:.8rem; opacity:.8; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
</style>
```

- [ ] **Step 3: `PromptPanel.svelte`**

```svelte
<script lang="ts">
  import { request } from "../stores";
</script>

<div class="field">
  <label class="label" for="prompt">Prompt</label>
  <textarea id="prompt" rows="3" bind:value={$request.prompt} placeholder="a lovely cat, oil painting"></textarea>
</div>
<div class="field">
  <label class="label" for="neg">Negative prompt</label>
  <textarea id="neg" rows="2" bind:value={$request.negative_prompt} placeholder="blurry, low quality"></textarea>
</div>

<style>
  textarea { width:100%; resize:vertical; font:inherit; padding:.4rem; box-sizing:border-box; }
</style>
```

- [ ] **Step 4: `SettingsPanel.svelte`** (the few well-defaulted parameters)

```svelte
<script lang="ts">
  import { request } from "../stores";
  import { SAMPLERS } from "../types";
</script>

<div class="grid">
  <label class="label">Steps
    <input type="number" min="1" max="150" bind:value={$request.steps} />
  </label>
  <label class="label">CFG
    <input type="number" min="1" max="30" step="0.5" bind:value={$request.cfg_scale} />
  </label>
  <label class="label">Sampler
    <select bind:value={$request.sampler}>
      {#each SAMPLERS as s}<option value={s}>{s}</option>{/each}
    </select>
  </label>
  <label class="label">Width
    <input type="number" min="64" max="2048" step="64" bind:value={$request.width} />
  </label>
  <label class="label">Height
    <input type="number" min="64" max="2048" step="64" bind:value={$request.height} />
  </label>
  <label class="label">Batch
    <input type="number" min="1" max="8" bind:value={$request.batch_count} />
  </label>
  <label class="label seed">Seed (-1 = random)
    <input type="number" bind:value={$request.seed} />
  </label>
</div>

<style>
  .grid { display:grid; grid-template-columns:1fr 1fr; gap:.5rem; }
  .label { display:flex; flex-direction:column; font-size:.75rem; gap:.2rem; }
  .seed { grid-column:1 / -1; }
  input, select { font:inherit; padding:.3rem; }
</style>
```

- [ ] **Step 5: Type-check and commit**

Run: `npm run check`
Expected: 0 errors.

```bash
git add -A
git commit -m "feat: control components (monitor, model picker, prompt, settings)" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 15: Components — GenerateBar, ImagePreview, HistoryStrip

**Files:**
- Create: `src/lib/components/GenerateBar.svelte`
- Create: `src/lib/components/ImagePreview.svelte`
- Create: `src/lib/components/HistoryStrip.svelte`

- [ ] **Step 1: `GenerateBar.svelte`** (button + progress + error box; owns the generate call)

```svelte
<script lang="ts">
  import { get } from "svelte/store";
  import { request, genStatus, history, currentImage } from "../stores";
  import { generate, cancelGeneration, imageSrc, listHistory } from "../api";

  async function run() {
    const req = get(request);
    if (!req.model_path) { genStatus.set({ kind: "error", message: "Select a model first." }); return; }
    if (!req.prompt.trim()) { genStatus.set({ kind: "error", message: "Enter a prompt." }); return; }
    genStatus.set({ kind: "running", progress: null });
    try {
      const item = await generate(req);
      currentImage.set(imageSrc(item.image_path));
      history.set(await listHistory());
      genStatus.set({ kind: "idle" });
    } catch (e) {
      genStatus.set({ kind: "error", message: String(e) });
    }
  }
  $: pct = $genStatus.kind === "running" && $genStatus.progress
    ? Math.round(($genStatus.progress.current_step / $genStatus.progress.total_steps) * 100) : 0;
</script>

<div class="bar">
  {#if $genStatus.kind === "running"}
    <div class="progress"><div class="fill" style="width:{pct}%"></div></div>
    <button class="btn-secondary" on:click={cancelGeneration}>Cancel</button>
  {:else}
    <button class="btn-primary" on:click={run}>Generate</button>
  {/if}
</div>

{#if $genStatus.kind === "error"}
  <div class="error" role="alert">{$genStatus.message}</div>
{/if}

<style>
  .bar { display:flex; gap:.5rem; align-items:center; }
  .btn-primary { flex:1; padding:.6rem; font-weight:600; }
  .progress { flex:1; height:12px; background:rgba(255,255,255,.1); border-radius:6px; overflow:hidden; }
  .fill { height:100%; background:var(--accent); transition:width .15s linear; }
  .error { margin-top:.5rem; padding:.5rem; border-radius:6px; background:rgba(255,80,80,.15);
    color:#ffb4b4; font-size:.8rem; white-space:pre-wrap; }
</style>
```

- [ ] **Step 2: Subscribe `GenerateBar` to progress events** — add to its `<script>`:

```svelte
  import { onMount } from "svelte";
  import { onProgress } from "../api";
  onMount(() => {
    const un = onProgress((p) => genStatus.update((s) => s.kind === "running" ? { kind: "running", progress: p } : s));
    return () => { un.then((f) => f()); };
  });
```

- [ ] **Step 3: `ImagePreview.svelte`**

```svelte
<script lang="ts">
  import { currentImage } from "../stores";
</script>

<div class="preview">
  {#if $currentImage}
    <img src={$currentImage} alt="generated result" />
  {:else}
    <p class="empty">Your generated image will appear here.</p>
  {/if}
</div>

<style>
  .preview { flex:1; display:flex; align-items:center; justify-content:center;
    background:rgba(0,0,0,.2); border-radius:8px; overflow:hidden; }
  img { max-width:100%; max-height:100%; object-fit:contain; }
  .empty { opacity:.5; }
</style>
```

- [ ] **Step 4: `HistoryStrip.svelte`** (click a thumbnail to view it and restore its settings)

```svelte
<script lang="ts">
  import { history, currentImage, request } from "../stores";
  import { imageSrc } from "../api";
  import type { GalleryItem } from "../types";
  function open(item: GalleryItem) {
    currentImage.set(imageSrc(item.image_path));
    request.set({ ...item.request });
  }
</script>

<div class="strip">
  {#each $history as item (item.id)}
    <button class="thumb" on:click={() => open(item)} title={item.request.prompt}>
      <img src={imageSrc(item.image_path)} alt={item.request.prompt} />
    </button>
  {:else}
    <span class="empty">No images yet this session.</span>
  {/each}
</div>

<style>
  .strip { display:flex; gap:.4rem; overflow-x:auto; padding:.4rem; min-height:64px; align-items:center; }
  .thumb { padding:0; border:1px solid var(--border); border-radius:6px; overflow:hidden;
    width:56px; height:56px; flex:0 0 auto; cursor:pointer; background:none; }
  .thumb img { width:100%; height:100%; object-fit:cover; }
  .empty { opacity:.5; font-size:.8rem; }
</style>
```

- [ ] **Step 5: Type-check and commit**

Run: `npm run check`
Expected: 0 errors.

```bash
git add -A
git commit -m "feat: generate bar, image preview, and history strip components" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 16: Assemble Layout A in App.svelte + global theme

**Files:**
- Modify: `src/App.svelte` (replace scaffold content)
- Modify: `src/app.css` (theme variables)

- [ ] **Step 1: Replace `src/App.svelte`**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { settings, request, history, sysStats } from "./lib/stores";
  import { getSettings, listHistory, onSystemStats } from "./lib/api";
  import ModelPicker from "./lib/components/ModelPicker.svelte";
  import PromptPanel from "./lib/components/PromptPanel.svelte";
  import SettingsPanel from "./lib/components/SettingsPanel.svelte";
  import GenerateBar from "./lib/components/GenerateBar.svelte";
  import ImagePreview from "./lib/components/ImagePreview.svelte";
  import HistoryStrip from "./lib/components/HistoryStrip.svelte";
  import ResourceMonitor from "./lib/components/ResourceMonitor.svelte";

  onMount(() => {
    (async () => {
      const cfg = await getSettings();
      settings.set(cfg);
      // seed the form with last-used params + default model if present
      request.set({ ...cfg.last_request, model_path: cfg.default_model_path ?? cfg.last_request.model_path });
      history.set(await listHistory());
    })();
    const un = onSystemStats((s) => sysStats.set(s));
    return () => { un.then((f) => f()); };
  });
</script>

<main class="app">
  <aside class="controls">
    <h1 class="brand">fridAI</h1>
    <ModelPicker />
    <PromptPanel />
    <SettingsPanel />
    <div class="spacer"></div>
    <GenerateBar />
  </aside>

  <section class="stage">
    <ImagePreview />
    <HistoryStrip />
  </section>
</main>
<ResourceMonitor />

<style>
  .app { display:flex; height:calc(100vh - 34px); }
  .controls { flex:0 0 340px; display:flex; flex-direction:column; gap:.8rem;
    padding:1rem; border-right:1px solid var(--border); overflow-y:auto; }
  .brand { margin:0 0 .5rem; font-size:1.2rem; }
  .spacer { flex:1; }
  .stage { flex:1; display:flex; flex-direction:column; padding:1rem; gap:.6rem; min-width:0; }
</style>
```

- [ ] **Step 2: Set a minimal dark theme in `src/app.css`** (append; keep scaffold resets)

```css
:root {
  --border: #333;
  --accent: #4f7cff;
  color-scheme: dark;
}
body { margin:0; background:#1b1b1f; color:#e8e8ea; font-family: system-ui, sans-serif; }
button { cursor:pointer; border-radius:6px; border:1px solid var(--border); background:#2a2a30; color:inherit; }
.btn-primary { background:var(--accent); border-color:var(--accent); color:#fff; }
.label { font-size:.75rem; opacity:.85; }
```

> If the scaffold uses `src/main.ts` mounting `App` and imports `app.css` there, no change is needed. If `app.css` isn't imported, add `import "./app.css";` to `src/main.ts`.

- [ ] **Step 3: Type-check, then run the app and verify the layout renders**

Run: `npm run check` → Expected: 0 errors.
Run: `npm run tauri dev` → Expected: window shows the left control panel (model/prompt/settings/generate), an empty center preview, an empty history strip, and the resource monitor at the bottom reflecting real CPU/RAM (and your RTX 3060 VRAM). Close when satisfied.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: assemble Layout A main screen and dark theme" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 17: Bundle the engine as a Tauri sidecar + build the AppImage

**Files:**
- Modify: `src-tauri/tauri.conf.json` (externalBin)
- Modify: `src-tauri/capabilities/default.json` (shell scope is NOT needed — we spawn via std::process; only confirm asset + dialog perms)

- [ ] **Step 1: Provide the sidecar binary with the target-triple suffix** — Tauri's `externalBin` expects `<name>-<target-triple>`. Determine your triple and copy the binary:

```bash
TRIPLE=$(rustc -Vv | sed -n 's/host: //p')
cp src-tauri/binaries/sd-cli "src-tauri/binaries/sd-cli-$TRIPLE"
echo "created src-tauri/binaries/sd-cli-$TRIPLE"
```

- [ ] **Step 2: Declare the sidecar in `src-tauri/tauri.conf.json`** — under `bundle`, add:

```json
"externalBin": ["binaries/sd-cli"]
```

> Note: `resolve_binary()` (Task 11) looks for `sd-cli` next to the executable, which is exactly where Tauri installs a resolved sidecar at runtime. No code change needed.

- [ ] **Step 3: Build the release bundle**

Run: `npm run tauri build`
Expected: an AppImage (and/or `.deb`) is produced under `src-tauri/target/release/bundle/`. Note the AppImage path.

- [ ] **Step 4: Smoke-test the bundled app**

Run the produced AppImage:
```bash
chmod +x src-tauri/target/release/bundle/appimage/*.AppImage
./src-tauri/target/release/bundle/appimage/*.AppImage
```
Expected: the app launches as a single file with no separate engine install; the resource monitor works.

- [ ] **Step 5: Commit** (binaries stay gitignored; only config changes are committed)

```bash
git add src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "build: bundle sd-cli as a Tauri sidecar for single-file install" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 18: End-to-end verification on the RTX 3060

This task has no code — it confirms the whole beta works on real hardware.

- [ ] **Step 1: Full generation run**

Launch the app (`npm run tauri dev` or the AppImage). Choose a local `.safetensors`/`.gguf` model, type a prompt, keep defaults (20 steps, 512×512), click **Generate**.
Expected: the progress bar advances as the engine samples; on completion the image appears in the center; a thumbnail is added to the history strip; the resource monitor shows VRAM rising while the model is loaded.

- [ ] **Step 2: History + settings restore**

Generate a second image with a different prompt/sampler. Click the first thumbnail.
Expected: the first image reloads in the preview and the control panel restores that image's settings.

- [ ] **Step 3: Persistence**

Confirm files exist:
```bash
ls ~/.local/share/fridai/gallery/   # expect <id>.png and <id>.json pairs
cat ~/.config/fridai/config.json     # expect last_request persisted
```
Expected: PNG + JSON sidecar per generation; config holds the last-used request.

- [ ] **Step 4: Error path (OOM)**

Set width/height high (e.g. 1536×1536) and a large batch to force CUDA OOM on 12 GB, click Generate.
Expected: no crash; a readable error box appears saying out of GPU memory and suggesting smaller dimensions/batch.

- [ ] **Step 5: Missing-engine path**

In Settings (or by temporarily renaming the binary), point at a non-existent engine and generate.
Expected: a clear "engine not found / set its path in Settings" message, no crash.

- [ ] **Step 6: Final commit / tag (optional)**

```bash
git commit --allow-empty -m "chore: fridAI beta verified end-to-end on RTX 3060" \
  -m "Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review Notes (author check vs. spec)

- **Spec coverage:** txt2img generation (Tasks 5,10,11,15,16), local model picker (14), few well-defaulted params (14), live progress (6,10,11,15), session history with remembered settings (8,15), resource monitor GPU/VRAM/CPU/RAM (9,11,14), files-only persistence (7,8,11), readable errors incl. OOM (10,11,15), bundled single-file install (17), tests (4–10) + manual E2E (18). All spec sections map to tasks.
- **Out-of-scope guardrails:** no downloads, LoRAs, img2img, training, or non-CUDA backends are implemented; seams (engine module, config, sidecar) leave room for them.
- **Type consistency:** Rust `GenerationRequest`/`Sampler`/`ProgressUpdate`/`GalleryItem`/`AppConfig` field names match their TS mirrors and the Tauri command signatures (`generate(request)`, `set_settings(config)`); event names `generation:progress` and `system:stats` match between `lib.rs`/`commands.rs` and `api.ts`.
- **Known reconciliation points (handled in-plan):** exact `sd-cli` flag spellings and progress-line format are verified against captured fixtures in Tasks 3/5/6; `sysinfo` API method names may need version-matching in Task 9.
```
