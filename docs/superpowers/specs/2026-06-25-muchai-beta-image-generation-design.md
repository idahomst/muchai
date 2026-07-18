# MuchAI — Beta (Phase 1) Design Spec

**Date:** 2026-06-25
**Status:** Approved design, pre-implementation
**Scope:** Phase 1 (beta) only. Phases 2–3 noted for context but explicitly out of scope.

## Context

Draw Things is an all-in-one desktop app for managing AI models, training, and
generating images. It has no real Linux equivalent. Existing Linux options
(ComfyUI, Automatic1111, etc.) are powerful but suffer from painful setup and
heavy Python dependency chains ("download half the internet"). The user — new
to app development but experienced with local LLM tooling (llama.cpp) — wants a
**simple-to-install, lightweight, native** desktop app for AI image generation
on Linux, with a clean path to a future macOS port.

This spec covers **Phase 1: a minimal but genuinely usable text-to-image app.**
The guiding principle is *start simple, leave clean seams for later phases*.

### Why this stack (decisions already made)

- **Engine: stable-diffusion.cpp** — the image-gen sibling of llama.cpp (ggml
  core). One native binary, no Python. Supports CUDA now and Vulkan/Metal/ROCm
  later — the only option that cleanly delivers the multi-vendor + Apple-Silicon
  future. Loads standard `.safetensors` and quantized `.gguf` models.
- **App framework: Tauri** (Rust backend + Svelte/TypeScript web UI). Uses the
  OS webview (no bundled browser → tiny binaries, unlike Electron). Produces a
  single AppImage on Linux and `.app`/`.dmg` on macOS.
- **Integration: subprocess.** The app spawns the `sd` CLI and parses its stdout
  for progress. Robust, simple, engine-version-independent. (Library/FFI binding
  is a possible future optimization, not needed now.)
- **Engine packaging: bundled** via Tauri's sidecar mechanism → self-contained,
  single-file install. No dependency hell. The only large files on disk are the
  models the user already owns.

## Goals (beta)

1. Type a prompt, pick a local model, click Generate, see the image.
2. A handful of well-defaulted parameters — a beginner can ignore all of them.
3. Live progress while generating.
4. Session history of generated images, each remembering its settings.
5. A small always-visible system resource monitor (GPU/VRAM/CPU/RAM).
6. Dead-simple install: one self-contained app file; point it at a model.
7. Robust error handling — never crash; surface engine errors readably.

## Non-Goals (Phase 2+)

Model downloading/browsing, LoRAs/VAEs, img2img/inpainting, ControlNet,
per-model recommended values, expandable inline parameter help, training,
non-CUDA backends (Vulkan/ROCm/Metal), persistent searchable gallery/library,
prompt presets/templates. The architecture must leave clean seams for these but
must not implement them.

## Architecture

```
┌──────────────────────────────────────────────────────┐
│  Tauri app (single self-contained binary)             │
│                                                        │
│  Web UI (Svelte + TS)         Rust backend (core)      │
│  ── Layout A main screen      ── generate command      │
│     left panel: model+params     spawn sidecar `sd`    │
│     center: image preview        parse stdout→progress │
│     bottom: history strip        write PNG + meta JSON │
│     resource monitor readout  ── settings load/save    │
│           │   ▲                ── system stats (NVML +  │
│        Tauri IPC                  sysinfo)             │
│           ▼   │                        │               │
│      (commands + events)               ▼               │
│                              stable-diffusion.cpp `sd` │
│                              (bundled sidecar binary)  │
└──────────────────────────────────────────────────────┘
```

### Components (each independently understandable/testable)

**Rust backend**
- `engine` — builds the `sd` command line from a `GenerationRequest`, spawns the
  bundled sidecar, streams stdout, parses progress, returns the output path.
  - `command_builder` — pure function: `GenerationRequest` → `Vec<String>` args.
    Unit-testable with no GPU. Supports a **dry-run** flag (build args, don't run).
  - `progress_parser` — pure function: a line of `sd` stdout → optional progress
    update (current step / total). Unit-testable against captured output.
- `gallery` — writes the output PNG to the gallery dir + a sidecar `.json` of
  all parameters; lists session items.
- `config` — load/save app settings (model path(s), last-used params, gallery
  dir) to a config file.
- `sysmon` — periodic system stats: GPU name/util/VRAM via **NVML** (`nvml-wrapper`
  crate), CPU/RAM via **sysinfo** crate. Emits to UI on an interval. Degrades
  gracefully if NVML is unavailable (e.g., non-NVIDIA) → GPU section hidden.

**Tauri command surface (UI ↔ backend)**
- `generate(request) -> result path` (emits `progress` events while running)
- `cancel_generation()`
- `list_history() -> [items]`
- `get_settings() / set_settings(settings)`
- `pick_model_file()` (native file dialog)
- system stats pushed via a `sysmon` event stream

**Svelte UI (Layout A: left controls + canvas + bottom history strip)**
- Left panel: model picker (file path + native picker button), prompt, negative
  prompt, settings group (Steps, CFG, Sampler, Width/Height, Seed, Batch count)
  with sane defaults, Generate button + progress bar.
- Center: large preview of current/last image.
- Bottom: horizontal thumbnail strip of this session's generations; click a
  thumbnail to view it and restore its settings.
- Resource monitor: compact readout (GPU util, VRAM used/free, CPU %, RAM
  used/free), always visible (e.g., footer/corner).

## Data Flow — one generation

1. UI gathers settings → invokes `generate(request)`.
2. `command_builder` produces `sd` args (e.g. `-m model -p "..." -n "..."
   --steps N --cfg-scale X --sampling-method ... -W w -H h -s seed -b batch
   -o <gallery>/<id>.png`).
3. `engine` spawns the sidecar `sd`, reads stdout line-by-line; `progress_parser`
   turns step lines into `progress` events → UI progress bar.
4. On success: `gallery` confirms the PNG and writes `<id>.json` (all params).
   `generate` returns the path.
5. UI displays the image and prepends a history thumbnail (with its settings).
6. On failure (non-zero exit / OOM / bad path): return a structured error; UI
   shows a readable box with the engine's stderr.

## Persistence (files only — no database)

- Images: `~/.local/share/muchai/gallery/<id>.png` + `<id>.json` sidecar.
- Config: platform config dir (e.g. `~/.config/muchai/config.json`) — sd model
  path(s), gallery dir, last-used parameters.
- macOS uses the platform-appropriate dirs (via Tauri/`directories` crate).

## Error Handling

- Missing/invalid model or engine → clear UI message, no crash.
- Non-zero exit / CUDA OOM → surface `sd` stderr in a readable error box. OOM is
  common on 12 GB VRAM at high resolution, so detect/name it and suggest lowering
  resolution or batch count.
- NVML unavailable → hide the GPU portion of the monitor; app still works.
- Generation is cancelable (kill the child process).

## Testing Strategy

- **Unit (no GPU, CI-friendly):**
  - `command_builder` — request → args, including dry-run.
  - `progress_parser` — captured `sd` stdout lines → progress updates, including
    malformed/edge lines.
  - `config` — round-trip load/save.
- **Integration:** dry-run path exercises UI↔backend wiring without executing
  the engine.
- **Manual E2E:** real generation on the user's RTX 3060 (12 GB) with a local
  `.safetensors`/`.gguf` model: progress updates, image saved, history works,
  resource monitor reflects load, OOM path produces a friendly error.

## Target / Reference Hardware

Primary dev/test machine: NVIDIA RTX 3060, 12 GB VRAM, Linux. CUDA backend for
the beta. Future multi-vendor (Vulkan/ROCm/Metal) is out of scope but the engine
choice already supports it.

## Open Implementation Details (resolve during planning, not blockers)

- Exact prebuilt-vs-self-built source of the bundled CUDA `sd` binary and how its
  CUDA runtime libraries are packaged into the AppImage.
- Final Svelte tooling choice (e.g. SvelteKit vs. plain Vite + Svelte) — lean
  toward the simplest that Tauri supports well.
- Exact `sd` CLI flag names/format (verify against the installed engine version
  when wiring `command_builder`).
```
