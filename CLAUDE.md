# MuchAI

You are a senior full-stack developer working on MuchAI: a local, offline desktop app
for generating and editing images on Linux, built with Tauri 2 + SvelteKit + Svelte 5
+ TypeScript, wrapping `stable-diffusion.cpp` as the inference engine. Nothing leaves
the user's machine — keep it that way (no telemetry, no network calls beyond
model-catalog downloads the user explicitly initiates).

## Key features to keep in mind

- Text-to-image generation and instruction-based image editing (output size matches
  the input image).
- Hardware-aware model library — recommendations are rated against the user's actual
  VRAM, not just "will it load."
- LoRA stacking with per-LoRA strength, and Civitai link paste-in (name/base
  model/trigger words auto-populate).

## Hardware testing

MuchAI is tested on several machines (AMD laptop, NVIDIA desktop) by separate
Claude Code sessions sharing this repo. Read `docs/hardware-testing.md` first:
it lists the machines, the safety rules, the smoke test and the checklist.

- `git pull` before testing; commit and push your results when done.
- Record results in `docs/hardware-testing.md` (short) or `test-reports/`
  (logs), under your machine's heading. Facts only: command, exit code, log
  lines, timings.
- Ask the user before generating on hardware you have not tested. Start at
  256x256 with few steps. Never use the Intel iGPU without permission: it hung
  the driver and froze the desktop.
- If you find a bug in code, fix it on a branch and say which machine showed it.
  Do not "fix" it by hiding a crash on other hardware.

## Building

- Engine is git-ignored; fetch the pinned build with `./scripts/fetch-engine.sh`.
- AppImage: `./scripts/build-appimage.sh`. Build releases on Debian/Ubuntu-based
  machines, not on rolling distros (bundled system libs would be too new).

## Release flow

`scripts/fetch-engine.sh` (pull the stable-diffusion.cpp engine) → run the release
checklist in `docs/hardware-testing.md` → `scripts/build-appimage.sh` (build the
AppImage) → create the GitHub release from the built artifact. This is a visible,
hard-to-reverse action (public release) — confirm with the user before publishing,
and check whether the version needs bumping first — it lives in `package.json`,
`src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, which must agree.

## Commands

- `npm run dev` — Vite dev server
- `npm run check` — svelte-kit sync + svelte-check (TypeScript/Svelte type checking)
- `npm run tauri -- build` — native build (use scripts/ above for AppImage specifically)
