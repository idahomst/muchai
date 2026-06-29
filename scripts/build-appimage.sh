#!/usr/bin/env bash
# Build a self-contained fridAI AppImage.
#
# The engine is now a Vulkan build (sd-cli + its .so siblings shipped as the
# `engine` resource dir, found via RUNPATH=$ORIGIN). Vulkan reaches the GPU
# through the host's ICD loader (libvulkan.so.1) and the vendor ICDs it loads,
# both of which are version-locked to the installed driver. So we must NOT ship
# our own libvulkan: a bundled copy would shadow the host loader and break GPU
# access. We also strip any NVIDIA/CUDA driver libs that linuxdeploy may drag in
# (the Vulkan engine shouldn't need them, but be defensive) so the AppImage is
# not vendor-locked. Everything stripped here is host-provided at runtime.
#
# Prereq: src-tauri/binaries/engine/ must be the prebuilt Vulkan engine bundle
# (sd-cli + all sibling .so files).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APPIMAGE_DIR="src-tauri/target/release/bundle/appimage"
APPDIR="$APPIMAGE_DIR/fridai.AppDir"
PLUGIN="$HOME/.cache/tauri/linuxdeploy-plugin-appimage.AppImage"

echo ">> tauri build (appimage)…"
npm run tauri build -- --bundles appimage

echo ">> stripping host-provided GPU libs from AppDir…"
# libvulkan.so* is the host ICD loader; libcuda/libnvidia-* are driver libs.
# All must come from the host, never the bundle.
find "$APPDIR/usr/lib" \
  \( -iname 'libvulkan.so*' \
     -o -iname 'libcuda.so*' \
     -o -iname 'libnvidia-*' \
     -o -iname 'libnvcuvid*' \) \
  -print -delete

echo ">> repacking AppImage without host GPU libs…"
( cd "$APPIMAGE_DIR" \
  && ARCH=x86_64 OUTPUT="fridai_0.1.0_amd64.AppImage" \
     APPIMAGE_EXTRACT_AND_RUN=1 \
     "$PLUGIN" --appdir fridai.AppDir )

echo ">> done:"
ls -lh "$APPIMAGE_DIR"/fridai_0.1.0_amd64.AppImage
