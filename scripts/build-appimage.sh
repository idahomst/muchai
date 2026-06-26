#!/usr/bin/env bash
# Build a self-contained fridAI AppImage.
#
# Tauri's linuxdeploy step bundles the CUDA *runtime* (libcudart/libcublas/
# libcublasLt) automatically — which we want. But it also drags in
# libcuda.so.1, the NVIDIA *driver* userspace library. That one MUST come from
# the host: it is version-locked to the running kernel module, so a bundled
# copy will mismatch (and fail) on any machine with a different driver. We strip
# it back out and repack so the host's libcuda is used at runtime.
#
# Prereq: src-tauri/binaries/sd-cli* must be the multi-arch CUDA engine build.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APPIMAGE_DIR="src-tauri/target/release/bundle/appimage"
APPDIR="$APPIMAGE_DIR/fridai.AppDir"
PLUGIN="$HOME/.cache/tauri/linuxdeploy-plugin-appimage.AppImage"

export PATH="/usr/local/cuda/bin:$PATH"

echo ">> tauri build (appimage)…"
npm run tauri build -- --bundles appimage

echo ">> stripping host-provided driver libs from AppDir…"
# libcuda.so.1 (and any libnvidia-* that may leak) come from the host driver.
find "$APPDIR/usr/lib" \
  \( -iname 'libcuda.so*' -o -iname 'libnvidia-*' -o -iname 'libnvcuvid*' \) \
  -print -delete

echo ">> repacking AppImage without driver libs…"
( cd "$APPIMAGE_DIR" \
  && ARCH=x86_64 OUTPUT="fridai_0.1.0_amd64.AppImage" \
     APPIMAGE_EXTRACT_AND_RUN=1 \
     "$PLUGIN" --appdir fridai.AppDir )

echo ">> done:"
ls -lh "$APPIMAGE_DIR"/fridai_0.1.0_amd64.AppImage
