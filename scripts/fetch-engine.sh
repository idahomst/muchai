#!/usr/bin/env bash
# Fetch the pinned stable-diffusion.cpp Vulkan engine bundle into
# src-tauri/binaries/engine/.
#
# WHY THIS IS PINNED: the engine (sd-cli + its .so siblings) is git-ignored and
# provisioned out-of-band, so nothing in the repo used to record *which* build
# was in place. An older build (commit 7b5f34d) had broken FLUX support: it
# produced a constant, input-independent image (a flat "yellow square") on both
# Vulkan and CPU regardless of prompt/steps/CFG. Commit b290693 fixes it. Pin
# the known-good revision here so a fresh checkout / CI / release build can never
# silently regress to a broken engine again. Bump ENGINE_REV + ENGINE_TAG +
# ENGINE_SHA256 together when intentionally upgrading (also bump
# BUILTIN_ENGINE_TAG in src-tauri/src/engine_release.rs), and re-verify a real
# generation.
#
# 2026-08-10: bumped b290693 -> bfbef5b. b290693 has a use-after-scope bug in
# prepare_image_generation_embeds (`empty_ref_images` declared inside the
# `if (use_ref_latent_img_cfg)` block, pointer read after it), taken by every
# run with CFG > 1 AND at least one reference image — i.e. every instruction
# edit. Fixed by leejet/stable-diffusion.cpp#1813 (b8bf676). bfbef5b is the
# build verified by hand through the in-app engine updater.
set -euo pipefail

# --- Pinned engine release (leejet/stable-diffusion.cpp GitHub Releases) --------
ENGINE_REV="bfbef5b"
ENGINE_TAG="master-813-bfbef5b"
ENGINE_ASSET="sd-master-bfbef5b-bin-Linux-Ubuntu-24.04-x86_64-vulkan.zip"
ENGINE_URL="https://github.com/leejet/stable-diffusion.cpp/releases/download/${ENGINE_TAG}/${ENGINE_ASSET}"
ENGINE_SHA256="7672553dccb9c4eeec850e1a7ad6f3c4bcde584ad6deae6648ed8b29c5cdbf7f"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/src-tauri/binaries/engine"

echo ">> fetching engine ${ENGINE_REV} (${ENGINE_TAG})"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
zip="$tmp/engine.zip"

echo ">> downloading $ENGINE_ASSET"
curl -fSL --retry 3 -o "$zip" "$ENGINE_URL"

echo ">> verifying sha256"
echo "${ENGINE_SHA256}  ${zip}" | sha256sum -c -

echo ">> extracting into a clean $DEST"
rm -rf "$DEST"
mkdir -p "$DEST"
unzip -q "$zip" -d "$DEST"

# Drop the standalone server we don't ship (the app only spawns sd-cli); keep the
# license/version .txt files for provenance and MIT compliance.
rm -f "$DEST/sd-server"

chmod +x "$DEST/sd-cli"

echo ">> verifying extracted engine reports the pinned revision"
# Capture without piping sd-cli directly into head: head closing the pipe early
# would send sd-cli SIGPIPE, which pipefail+set -e turns into a spurious abort.
# --help exits non-zero on some builds, so ignore its status; we assert on text.
help_out="$(LD_LIBRARY_PATH="$DEST" "$DEST/sd-cli" --help 2>&1 || true)"
got="$(printf '%s\n' "$help_out" | head -1)"
if ! printf '%s\n' "$got" | grep -q "commit ${ENGINE_REV}"; then
  echo "ERROR: extracted engine does not report commit ${ENGINE_REV}" >&2
  echo "       got: $got" >&2
  exit 1
fi

echo ">> done: $got"
echo "   engine installed at $DEST"
