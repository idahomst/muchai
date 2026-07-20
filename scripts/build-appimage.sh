#!/usr/bin/env bash
# Build a self-contained MuchAI AppImage.
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
# We ALSO strip a small set of "host-coupled" system libraries so the AppImage
# runs across Debian/Ubuntu versions instead of only the build machine's distro:
#   * libgcrypt — bundling it without its peer libgpg-error (which the AppImage
#     excludelist deliberately keeps host-provided) makes the newer bundled
#     libgcrypt resolve gpgrt_* symbols against an OLDER host libgpg-error and
#     crash ("undefined symbol: gpgrt_add_post_log_func, version GPG_ERROR_1.0"
#     seen on Linux Mint 21.x). Removing our copy lets the consistent host pair
#     (libgcrypt + libgpg-error, present on every Debian/Ubuntu desktop) load.
#   * libwayland-client — the one AppImage-excludelist lib present in our bundle;
#     the Wayland client must match the host compositor, so it's host-provided.
# We deliberately do NOT strip libstdc++/libfreetype/libharfbuzz etc.: the
# bundle is built on a newer distro, so its (newer) webkit2gtk may need newer
# symbols from those than an old host ships — stripping them would break the
# other way. If a NEW "undefined symbol" appears on some target, add that one
# specific lib here rather than blanket-stripping.
#
# Prereq: src-tauri/binaries/engine/ must be the prebuilt Vulkan engine bundle
# (sd-cli + all sibling .so files). Populate it with the pinned, known-good
# revision by running ./scripts/fetch-engine.sh first — do NOT hand-place an
# arbitrary build (an older engine silently broke FLUX; see that script's header).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APPIMAGE_DIR="src-tauri/target/release/bundle/appimage"
APPDIR="$APPIMAGE_DIR/muchai.AppDir"
PLUGIN="$HOME/.cache/tauri/linuxdeploy-plugin-appimage.AppImage"

echo ">> tauri build (appimage)…"
npm run tauri build -- --bundles appimage

echo ">> stripping host-provided / host-coupled libs from AppDir…"
# libvulkan.so* is the host ICD loader; libcuda/libnvidia-* are driver libs.
# libgcrypt + libwayland-client must come from the host too (see header). All of
# these must come from the host, never the bundle.
find "$APPDIR/usr/lib" \
  \( -iname 'libvulkan.so*' \
     -o -iname 'libcuda.so*' \
     -o -iname 'libnvidia-*' \
     -o -iname 'libnvcuvid*' \
     -o -iname 'libgcrypt.so*' \
     -o -iname 'libwayland-client.so*' \) \
  -print -delete

echo ">> repacking AppImage without host GPU libs…"
( cd "$APPIMAGE_DIR" \
  && ARCH=x86_64 OUTPUT="muchai_0.1.0_amd64.AppImage" \
     APPIMAGE_EXTRACT_AND_RUN=1 \
     "$PLUGIN" --appdir muchai.AppDir )

echo ">> done:"
ls -lh "$APPIMAGE_DIR"/muchai_0.1.0_amd64.AppImage
