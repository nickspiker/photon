#!/bin/bash
# Windows dev: cross-compile (--features development) for x86_64-pc-windows-gnu + Ed25519 sign.
# Unlike scripts/dev.sh (which builds for the HOST and installs to ~/.local/bin so you can run it
# here), this cross-builds the Windows .exe on a non-Windows host — there's nothing to install/run
# locally, so it just signs the .exe in-tree and prints its path. Copy that to the Windows box to run.
#
# Prereqs (one-time): rustup target add x86_64-pc-windows-gnu  + the mingw-w64 cross linker
# (Fedora: dnf install mingw64-gcc; Debian/Ubuntu: apt install gcc-mingw-w64-x86-64).
set -e
cd "$(dirname "$0")/.."
source scripts/lib/sign.sh

TARGET=x86_64-pc-windows-gnu
BIN="target/$TARGET/debug/photon-messenger.exe"

echo "Building Windows dev binary ($TARGET, --features development)..."
cargo build --features development --target "$TARGET"

# sign_binary knows the windows-gnu .exe layout (see scripts/lib/sign.sh). The signer is a host tool;
# it builds itself once if this tree hasn't produced one yet.
sign_binary debug "$TARGET"

echo ""
echo "✓ Signed Windows dev binary:"
echo "    $(pwd)/$BIN"
echo "  Copy it to the Windows machine and run it there (it can't execute on this host)."
