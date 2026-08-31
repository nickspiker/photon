#!/bin/bash
# Publish a Windows ARM64 dev build (cross-compiled from Linux via llvm-mingw, logging on) to the R2 dev channel: build -> sign -> upload binary.
# Companion to dev-windows-x86.sh. The toolchain env mirrors deploy.sh's aarch64-pc-windows-gnullvm release recipe (llvm-mingw is BOTH the C compiler cc-rs auto-detects on PATH and the linker).
# Binary-only, like dev-linux-arm64.sh: install-development.ps1 stays an x86_64 artefact (its injected hash names one binary) — ARM64 dev installs are a hand-copied .exe, and the in-binary self-verify covers integrity.
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/sign.sh
source scripts/lib/publish.sh
source scripts/lib/github.sh
source scripts/lib/manifest.sh

WINARM_MINGW="/mnt/Harbor/Code/llvm-mingw"
if [ ! -x "$WINARM_MINGW/bin/aarch64-w64-mingw32-clang" ]; then
    echo "ERROR: llvm-mingw toolchain not found at $WINARM_MINGW — required for the Windows ARM64 target."
    echo "       Install from github.com/mstorsjo/llvm-mingw (ucrt build)."
    exit 1
fi

# Refuse-dirty + patch-bump + commit BEFORE the build, so the binary embeds a clean HEAD whose commit is exactly what the signed manifest claims (docs/updates.md).
manifest_begin_dev_publish "windows-arm64"

echo "Building Windows ARM64 development binary..."
PATH="$WINARM_MINGW/bin:$PATH" \
CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER="$WINARM_MINGW/bin/aarch64-w64-mingw32-clang" \
    cargo build --target aarch64-pc-windows-gnullvm --features development
sign_binary debug aarch64-pc-windows-gnullvm

echo "Uploading to R2 (primary)..."
publish_r2 "photon-messenger-windows-arm64-development.exe" target/aarch64-pc-windows-gnullvm/debug/photon-messenger.exe

echo "Publishing dev manifest row..."
manifest_publish_dev_row "Windows" "arm64" "photon-messenger-windows-arm64-development.exe" target/aarch64-pc-windows-gnullvm/debug/photon-messenger.exe

echo "Mirroring to GitHub Releases (dev)..."
publish_github_dev "photon-messenger-windows-arm64-development.exe" target/aarch64-pc-windows-gnullvm/debug/photon-messenger.exe

echo ""
echo "Windows ARM64 dev published:"
echo "  $R2_BASE_URL/photon-messenger-windows-arm64-development.exe"
echo "  Copy the .exe to the Windows ARM64 machine and run it (self-verifies on launch)."

# Publish landed — bump the patch + commit, opening the next dev line (publish-current-then-bump).
manifest_end_dev_publish
