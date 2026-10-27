# Hardware testing notebook

MuchAI is tested on several machines by separate Claude Code sessions that share
this repo through git. Each session runs checks on its own hardware and records
the result here. Pull before you start, commit and push when you finish.

The engine is whatever `scripts/fetch-engine.sh` pins (`ENGINE_TAG`). Engines
change between test runs, so every result heading names the one it ran on.

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

On a machine with more than one GPU, add `--backend vulkanN` for the GPU under
test. Without it the engine picks its own device; on the desktop `Vulkan0` is
the Intel iGPU (see the safety rules). `sd-cli --list-devices` prints the names.

Record: exit code, the `auto-fit` lines, the `ggml_vulkan` device line, time
for sampling, and whether the image looks right. For a crash, add the top of
`coredumpctl gdb <pid> -A "-batch -ex bt"`.

## Current release: 1.7.1

What this release changes, and so what the checklist's "release-specific"
items mean right now:

- Engine `master-1015-e41fbcf`.
- Qwen-Image 2.1 and Flux.3 klein support (recipes `qwen-image-2.1`, `flux3`).
  Both generate and edit: the reference image is optional.
- Catalog: Flux.3 klein UD-Q4_K_XL and Q8_0, Qwen-Image 2.1 Q4_K_M and Q8_0.
  FLUX.1, Qwen-Image and Qwen-Image-Edit entries removed (their recipes stay).
- `--llm_vision` is passed only with a reference image.
- The gallery no longer records a reference image the run did not use.

Qwen-Image 2.1 files used so far (install under
`~/.local/share/muchai/models/`):

| Role | File | Source |
| --- | --- | --- |
| Diffusion | `qwen-image-2.1-Q8_0.gguf` (7.1 GB) | `unsloth/Qwen-Image-2.1-GGUF` |
| VAE | `qwen_image_2.1_vae_bf16.safetensors` | `Comfy-Org/Qwen-Image-2.1`, `vae/` |
| Text encoder (GGUF) | `Qwen3VL-8B-Instruct-Q8_0.gguf` (8.1 GB) | `Qwen/Qwen3-VL-8B-Instruct-GGUF` |
| Vision (reference image) | `mmproj-Qwen3VL-8B-Instruct-F16.gguf` | `Qwen/Qwen3-VL-8B-Instruct-GGUF` |
| Text encoder (int8, crashes on RADV) | `qwen3vl_8b_int8_convrot.safetensors` | `Comfy-Org/Qwen-Image-2.1`, `text_encoders/` |

Flux.3 klein files (pooled under `shared/flux3/`):

| Role | File | Source |
| --- | --- | --- |
| Diffusion | `flux-3-image-klein-UD-Q4_K_XL.gguf` (3.9 GB) | `unsloth/flux-3-image-klein-GGUF` |
| VAE | `flux2-vae.safetensors` | `Comfy-Org/flux2-dev`, `split_files/vae/` |
| Text encoder | `Qwen3VL-4B-Instruct-Q8_0.gguf` (4.3 GB) | `Qwen/Qwen3-VL-4B-Instruct-GGUF` |

No vision file: Flux.3 reads the reference image through the VAE.

When the next release starts, replace this section; keep the results below.

## Results

### Laptop, AMD 8060S (2026-10-26, engine master-995-fd21ecf)

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

### Desktop, RTX 3060 (2026-10-26, engine master-1015-e41fbcf)

RTX 3060 12 GB (`Vulkan1`, NVIDIA 615.71.10), 31 GB RAM. Every run used
`--backend vulkan1`. Command and full log of each run are in
`test-reports/desktop-rtx3060-*.log`.

- `./scripts/fetch-engine.sh`: exit 0, checksum OK, reports `commit e41fbcf`.
- Qwen-Image 2.1 Q8_0 + bf16 VAE + GGUF Qwen3-VL-8B Q8_0 encoder. Auto-fit,
  same on every run:
  ```
  DiT          params   7286 MiB, compute reserve  2048 MiB -> compute Vulkan1, params Vulkan1
  Conditioner  params   8300 MiB, compute reserve  2048 MiB -> compute Vulkan1, params cpu
  VAE          params    644 MiB, compute reserve  1024 MiB -> compute Vulkan1, params cpu
  auto-fit: --backend "vulkan1" --params-backend "te=cpu,vae=cpu"
  ```
  (Conditioner is 9405 MiB on the reference-image run, with the mmproj.)

  | Run | Exit | Sampling | Wall | Image |
  | --- | --- | --- | --- | --- |
  | 256x256, 4 steps | 0 | 4.64 s | 13.8 s | apple, soft (expected at 4 steps) |
  | 512x512, 8 steps | 0 | 10.84 s | 18.7 s | clean apple |
  | 512x512, 8 steps, `--ref-image` (the 512 apple) + `--llm_vision` mmproj F16, "make the apple green" | 0 | 12.66 s | 23.2 s | same apple, green |

- Same model with the int8 safetensors encoder
  (`qwen3vl_8b_int8_convrot.safetensors`), 256x256 / 4 steps: **exit 135,
  SIGBUS** after 36 s, in the first text-encoder graph:
  `ggml_vk_tensor_subbuffer` <- `ggml_vk_op_f32` <- `ggml_vk_add` <-
  `ggml_vk_build_graph` <- `ggml_backend_vk_graph_compute` <- ... <-
  `LLMEmbedder::encode_prompt` (full trace:
  `test-reports/desktop-rtx3060-int8enc-sigbus-backtrace.txt`). It also fails on
  the laptop (SIGSEGV, also in `ggml_backend_vk_graph_compute`), so it is not
  specific to RADV. `--backend te=cpu` was not tried here.
- Qwen-Image 2.1 at the new recipe defaults (20 steps, CFG 6.0, euler),
  512x512, no vision file: exit 0, sampling 80.50 s, wall 88.4 s, clean apple
  (`desktop-rtx3060-qwen-image-2.1-defaults-512x512-20steps-cfg6.log`).
- Flux.3 klein UD-Q4_K_XL + flux2 VAE + Qwen3-VL-4B Q8_0, `--cfg-scale 1.0`,
  `euler`, no vision file. Engine prints `Version: Flux.3 Image Klein` and
  `running in Flux.3 FLOW mode`; auto-fit put everything on `Vulkan1`.

  | Run | Exit | Sampling | Wall | Image |
  | --- | --- | --- | --- | --- |
  | 256x256, 4 steps | 0 | 1.82 s | 4.8 s | apple |
  | 512x512, 16 steps | 0 | 11.71 s | 14.6 s | clean apple |
  | 512x512, 16 steps, `--ref-image` (the 512 apple), "make the apple green" | 0 | 24.31 s | 27.8 s | same apple, green |

- Not run: the gallery check with a non-edit model, and the AppImage check. Both
  need the app's window. Not downloaded or run: Flux.3 Q8_0, Qwen-Image 2.1
  Q4_K_M.

## Checklist for a new machine or release

Use models that suit the machine; the release-specific items come from
"Current release" above.

- [ ] `./scripts/fetch-engine.sh` succeeds (checksum matches)
- [ ] Smoke test at 256x256 / 4 steps
- [ ] Same at 512x512 / 8 steps
- [ ] A model larger than the machine's VRAM (or memory budget): record the
      `auto-fit` placement lines and the time
- [ ] Each model family the release adds or changes, at the sizes above; for an
      edit-capable model, once without a reference image and once with one
- [ ] A non-edit model: the gallery entry shows no "Editing ..." row
- [ ] AppImage starts, loads the bundled engine, and generates
