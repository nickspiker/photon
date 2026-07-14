#!/bin/bash
# Windows dev: cross-compile (--features development) for x86_64-pc-windows-gnu + Ed25519 sign, then leave the signed .exe in-tree for you to hand-copy to a Windows box (it can't run on this host).
#
# Three Windows paths, don't confuse them:
#   scripts/build-windows.sh        (this) build + sign locally; copy the .exe over yourself.
#   scripts/publish/dev-windows.sh  build + sign + UPLOAD to the R2 dev channel + inject the binary's SHA256 into the PowerShell installer.
#   deploy.sh                       full release: every target, signed, uploaded, version-bumped.
#
# The Ed25519 signature appended to the .exe is the trust check — the binary self-verifies it at every launch (the "SIGNATURE CHECK PASSED" line).
# The .sha256 sidecar the signer also emits is ONLY for the installer path (Windows Defender blocks running a fresh download, so the installer verifies the hash BEFORE execution); for a hand-copied dev .exe it's unused — the in-binary self-verify covers integrity.
#
# Prereqs (one-time): rustup target add x86_64-pc-windows-gnu  + the mingw-w64 cross linker (Fedora: dnf install mingw64-gcc; Debian/Ubuntu: apt install gcc-mingw-w64-x86-64).
set -e
cd "$(dirname "$0")/.."
source scripts/lib/sign.sh

TARGET=x86_64-pc-windows-gnu
BIN="target/$TARGET/debug/photon-messenger.exe"

echo "Building Windows dev binary ($TARGET, --features development)..."
cargo build --features development --target "$TARGET"

# sign_binary knows the windows-gnu .exe layout (see scripts/lib/sign.sh). The signer is a host tool; it builds itself once if this tree hasn't produced one yet.
sign_binary debug "$TARGET"

echo ""
echo "✓ Signed Windows dev binary:"
echo "    $(pwd)/$BIN"
echo "  Copy it to the Windows machine and run it there (it can't execute on this host)."
