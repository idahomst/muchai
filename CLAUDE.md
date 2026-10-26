# MuchAI

Tauri (Rust) + Svelte desktop app that drives stable-diffusion.cpp.

## Hardware testing

MuchAI is tested on several machines (AMD laptop, NVIDIA desktop) by separate
Claude Code sessions sharing this repo. Read `docs/hardware-testing.md` first:
it lists the machines, the safety rules and the smoke test.

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
