# Hardware testing notebook

MuchAI is tested on several machines by separate Claude Code sessions that share
this repo through git. Each session runs checks on its own hardware and records
the result here. Pull before you start, commit and push when you finish.

Engine under test: stable-diffusion.cpp `master-995-fd21ecf` (Vulkan).

## Machines

| Machine | GPU | Memory | Distro | Notes |
| --- | --- | --- | --- | --- |
| Laptop | AMD Radeon 8060S (RADV, Strix Halo) | 124 GB unified | CachyOS (rolling) | `uma: 1`; Vulkan via RADV |
| Desktop | NVIDIA RTX 3060 | dedicated VRAM | Linux Mint Debian Edition | release AppImages are built here |
| Desktop (iGPU) | Intel iGPU | shared | same PC | **hung the GPU driver and froze the PC** on a test generation |

## Safety rules

- Ask the user before any generation on a machine you have not tested before.
- Start tiny: 256x256, 4 steps. Raise size and steps only after that works.
- Never run a generation on the Intel iGPU without explicit permission.
- A segfault or hang is a result, not something to retry in a loop.

## Smoke test

Run from the repo root after `./scripts/fetch-engine.sh`. Replace the paths
with models installed on that machine (`~/.local/share/muchai/models/`).

```
src-tauri/binaries/engine/sd-cli \
  --diffusion-model <model.gguf> --vae <vae> --llm <text-encoder> \
  --vae-tiling --flash-attn auto,diffusion=on \
  -p "a red apple on a table" -W 256 -H 256 --steps 4 --cfg-scale 1.0 \
  --sampling-method euler_a -v -o /tmp/smoke.png
```

Record: exit code, the `auto-fit` lines, the `ggml_vulkan` device line, time
for sampling, and whether the image looks right. For a crash, add the top of
`coredumpctl gdb <pid> -A "-batch -ex bt"`.

## Results

### Laptop, AMD 8060S (2026-10-26)

- Qwen-Image 2.1 Q8_0 + bf16 VAE + **GGUF** Qwen3-VL-8B Q8_0 encoder: works.
  256x256/4 steps 5 s; 512x512/8 steps 13 s.
- Same model with the int8 safetensors encoder
  (`qwen3vl_8b_int8_convrot.safetensors`): **segfault** in
  `ggml_vk_tensors_overlap` <- `ggml_backend_vk_graph_compute` during the
  first text-encoder graph. Works with `--backend te=cpu`. Use a GGUF encoder.
- `[W] auto-fit: no GPU devices; using the default backend`
  (`backend_fit.cpp:444`): auto-fit does not see the UMA device. Runs still
  work. Upstream issue, not reported yet.
- `[W] conv3d direct auto: Vulkan0 -> off (probe output check failed)`:
  harmless.
- AppImage build needs `NO_STRIP=1` (linuxdeploy's strip cannot read
  `.relr.dyn` on rolling distros); `scripts/build-appimage.sh` sets it.

### Desktop, RTX 3060

Not recorded yet. Run the smoke test and the checklist below.

## Checklist for a new machine or release

- [ ] `./scripts/fetch-engine.sh` succeeds (checksum matches)
- [ ] Smoke test at 256x256 / 4 steps
- [ ] Same at 512x512 / 8 steps
- [ ] Qwen-Image 2.1 with the GGUF encoder (optional reference image: run
      once without a reference, once with one)
- [ ] A non-edit model (e.g. Z-Image Turbo): the gallery entry shows no
      "Editing ..." row
- [ ] AppImage starts, loads the bundled engine, and generates
