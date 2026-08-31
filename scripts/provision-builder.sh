#!/bin/bash
# Provision a fresh Linux x86_64 box as a photon build machine (Android + Linux targets).
#
# WHY THIS EXISTS: photon is not self-contained — it needs ELEVEN sibling repos as `../` path dependencies. A `git clone photon && cargo build` on a fresh box fails immediately. This script reproduces the whole tree, which is what a desktop accumulates by hand over months. (winit and softbuffer patches resolve on their own now — Cargo.toml points them at git forks.)
#
# WHAT THIS DOES NOT DO: no signing keys, no publishing. It produces UNSIGNED artefacts. The Ed25519 signing key, the Apple codesign cert and the TOKEN APK keystore stay wherever you keep them; see the notes at the end for what a real publish additionally needs.
#
# Usage:  bash provision-builder.sh [workdir]     (default workdir: $HOME/Code)
#
# Targets this box will handle: Linux x86_64 (native), Android arm64. NOT macOS — that needs osxcross + an Apple SDK; build macOS natively on a Mac instead.

set -euo pipefail

WORKDIR="${1:-$HOME/Code}"
GH="https://github.com/nickspiker"

# The NDK version is PINNED because scripts/lib/android-env.sh hardcodes this exact path. Taking "the latest NDK" silently breaks that script.
NDK_VERSION="25.2.9519653"
CMDLINE_TOOLS_URL="https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip"

# Sibling repos pinned to the commits verified to compile against photon @ af2465a (2026-08-30, full signed Android build + desktop tests green on this exact set). Pinning matters: cloning everything at main HEAD produced 22 type errors on the first attempt (RosterEntry/BindRequest drift in fgtw, VsfHeader in vsf). Bump these deliberately, not by accident.
declare -A SIBLINGS=(
  [vsf]=6669859        [fgtw]=9b30baf       [tohu]=f8c3a95
  [fluor]=d9e3ba9      [chirp]=8d4f657      [kete]=2f4ef92
  [manifestus]=5c1b214 [nunc]=f1dbc6b       [ihi]=ac72c54
  [rarangi]=8bd3fc7    [spirix]=af8e00e
)

say() { printf '\n\033[1;36m══ %s\033[0m\n' "$*"; }
warn() { printf '\033[1;33m!! %s\033[0m\n' "$*"; }

# ─────────────────────────────────────────────────────────────────────────────
say "System packages"
# mold is the linker .cargo/config.toml expects for fast native Linux links. clang/cmake/pkg-config are needed by the C build deps (blake3, ring, pqcrypto). The x11/wayland/alsa dev headers back the Linux host build of fluor.
if command -v apt-get >/dev/null; then
  sudo apt-get update -qq
  sudo apt-get install -y -qq \
    build-essential clang cmake pkg-config mold git curl unzip zip \
    libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxkbcommon-dev \
    libwayland-dev libasound2-dev libssl-dev \
    openjdk-21-jdk b3sum
elif command -v dnf >/dev/null; then
  sudo dnf install -y -q \
    gcc gcc-c++ clang cmake pkgconf-pkg-config mold git curl unzip zip \
    libX11-devel libXcursor-devel libXrandr-devel libXi-devel libxkbcommon-devel \
    wayland-devel alsa-lib-devel openssl-devel \
    java-21-openjdk-devel b3sum
else
  warn "Unknown package manager — install the build deps manually (see apt list above)."
fi

# ─────────────────────────────────────────────────────────────────────────────
say "Rust toolchain"
if ! command -v cargo >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
fi
# shellcheck disable=SC1091
source "$HOME/.cargo/env"
rustup target add aarch64-linux-android

# ─────────────────────────────────────────────────────────────────────────────
say "Source tree ($WORKDIR)"
mkdir -p "$WORKDIR"
cd "$WORKDIR"

clone_at() {  # clone_at <name> <commit>
  local name="$1" commit="$2"
  if [ -d "$name/.git" ]; then
    git -C "$name" fetch --quiet origin
  else
    git clone --quiet "$GH/$name.git" "$name"
  fi
  git -C "$name" checkout --quiet "$commit"
  printf '  %-12s %s\n' "$name" "$commit"
}

[ -d photon/.git ] || git clone --quiet "$GH/photon.git" photon
for name in "${!SIBLINGS[@]}"; do clone_at "$name" "${SIBLINGS[$name]}"; done

# winit-patched: RETIRED 2026-08-30. The vendored winit (Windows dark-mode integration neutralized) now lives at github.com/nickspiker/winit branch photon-patched, and photon's [patch.crates-io] points there — cargo resolves it with no sibling directory. The old reconstruct-from-upstream block that lived here is gone with it.

# ─────────────────────────────────────────────────────────────────────────────
say "Android SDK + NDK $NDK_VERSION"
# android-env.sh looks for ~/android-sdk or ~/Android/Sdk and hardcodes /home/nick. Using username `nick` on this box keeps that script working unmodified.
export ANDROID_HOME="$HOME/android-sdk"
if [ ! -d "$ANDROID_HOME/cmdline-tools/latest" ]; then
  mkdir -p "$ANDROID_HOME/cmdline-tools"
  curl -sSfL "$CMDLINE_TOOLS_URL" -o /tmp/cmdline-tools.zip
  unzip -q /tmp/cmdline-tools.zip -d "$ANDROID_HOME/cmdline-tools"
  mv "$ANDROID_HOME/cmdline-tools/cmdline-tools" "$ANDROID_HOME/cmdline-tools/latest"
  rm -f /tmp/cmdline-tools.zip
fi
SDKMGR="$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager"
yes 2>/dev/null | "$SDKMGR" --licenses >/dev/null 2>&1 || true
"$SDKMGR" --install "platform-tools" "ndk;$NDK_VERSION" >/dev/null

# ─────────────────────────────────────────────────────────────────────────────
say "Verify: photon compiles"
cd "$WORKDIR/photon"
if cargo check --features development 2>&1 | tail -5; then
  echo
  echo "Provisioning complete."
else
  warn "cargo check failed — see output above."
  exit 1
fi

cat <<'NOTES'

────────────────────────────────────────────────────────────────────────
WHAT THIS BOX CAN DO NOW
  cargo build --release                                  Linux x86_64
  cargo build --release --lib --target aarch64-linux-android    the Android .so

WHAT IT CANNOT DO WITHOUT SECRETS
  Signed artefacts and any publish need three things that live on the desktop:
    - the Ed25519 photon signing key   (PHOTON_SIGNING_KEY, or the keys dir)
    - the TOKEN APK keystore + password (TOKEN_KEYSTORE_PATH / _PASSWORD)
    - google-services.json              (Firebase; android.sh copies it from the keys dir)
  scripts/lib/keystore.sh resolves these from /mnt/Harbor/Code/keys,
  /mnt/Chiton/MEGA/Code/keys, or ~/MEGA/code/keys, and reads the password from the
  GNOME keyring via secret-tool.

  Gradle REFUSES to build a release APK without the keystore env — deliberately: a
  fallback key produces an APK that cannot update the installed app and gets a
  different ANDROID_ID, breaking the shared identity that key exists for.

  The TOKEN key signs every app in the family. Decide deliberately whether it belongs
  on a rented box. A middle path: build the .so here (no secrets), rsync it to a
  trusted machine, run Gradle there — android.sh splits cleanly at the copy into
  android/app/src/main/jniLibs/arm64-v8a/.

  NOT macOS: needs osxcross + an Apple SDK. Build natively on the Mac instead.
────────────────────────────────────────────────────────────────────────
NOTES
