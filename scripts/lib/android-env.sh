# Sourced, not executed. Sets up the Android NDK + cross-toolchain env for an aarch64-linux-android cargo build. Detects ANDROID_HOME, then derives the NDK from it (was hardcoded + drifted across the old scripts), plus the ring-crate clang symlinks and the host build flags that build.rs needs.

# SDK location: $ANDROID_HOME wins if already exported (a box that keeps it elsewhere), then the usual per-host defaults. macOS installs land in ~/Library/Android/sdk (Android Studio) or ~/android-sdk (a bare cmdline-tools install); Linux used the $HOME forms below, which were hardcoded to /home/nick until the MacBook build needed them (2026-07-27).
if [ -n "$ANDROID_HOME" ] && [ -d "$ANDROID_HOME/ndk" ]; then
    :  # caller-provided, keep it
elif [ -d "$HOME/android-sdk/ndk" ]; then
    export ANDROID_HOME="$HOME/android-sdk"
elif [ -d "$HOME/Android/Sdk/ndk" ]; then
    export ANDROID_HOME="$HOME/Android/Sdk"
elif [ -d "$HOME/Library/Android/sdk/ndk" ]; then
    export ANDROID_HOME="$HOME/Library/Android/sdk"
else
    echo "ERROR: Cannot find Android SDK (looked in \$ANDROID_HOME, $HOME/android-sdk, $HOME/Android/Sdk, $HOME/Library/Android/sdk)"
    exit 1
fi

export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/25.2.9519653"
# The NDK ships one prebuilt toolchain per host OS. Note the macOS one is named darwin-x86_64 even on Apple Silicon (it runs via Rosetta), so probe rather than assuming the host arch.
for _host in linux-x86_64 darwin-x86_64 darwin-arm64; do
    if [ -d "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$_host/bin" ]; then
        NDK_BIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$_host/bin"
        break
    fi
done
if [ -z "$NDK_BIN" ]; then
    echo "ERROR: No NDK prebuilt toolchain under $ANDROID_NDK_HOME/toolchains/llvm/prebuilt/"
    exit 1
fi
export PATH="$NDK_BIN:$PATH"

# The ring crate expects `aarch64-linux-android-clang` without the API-level suffix.
if [ ! -f "$NDK_BIN/aarch64-linux-android-clang" ]; then
    ln -sf aarch64-linux-android21-clang "$NDK_BIN/aarch64-linux-android-clang"
    ln -sf aarch64-linux-android21-clang++ "$NDK_BIN/aarch64-linux-android-clang++"
fi

# Android ARM64 target (the only Android target).
export CC_aarch64_linux_android="$NDK_BIN/aarch64-linux-android21-clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_BIN/aarch64-linux-android21-clang"

# Host build flags so the build.rs / proc-macro compiles use the fast local toolchain.
# Host-specific: the mold linker flags are x86_64-Linux only — setting CC/CXX to a bare `clang` on macOS would shadow the NDK wrappers' own host compiler resolution, and mold isn't there at all. On macOS the system clang from the Command Line Tools is already the right host compiler, so leave CC/CXX unset and let cargo/cc-rs find it.
if [ "$(uname -s)" = "Linux" ]; then
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="clang"
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"
    export CC="clang"
    export CXX="clang++"
else
    # macOS host. Putting NDK_BIN on PATH (needed so the ring crate finds its wrappers) also shadows Apple's clang — so the HOST half of the build (build.rs, proc-macros) tried to link Mach-O with the NDK's clang-14 and died on -lSystem / MacOSX.sdk.
    # Pin the host linker to Apple's clang by absolute path; the Android target keeps using the NDK wrapper set in CC_aarch64_linux_android above.
    _host_cc="$(xcrun --find clang 2>/dev/null || echo /usr/bin/clang)"
    export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="$_host_cc"
    export CC_aarch64_apple_darwin="$_host_cc"
    export CXX_aarch64_apple_darwin="$(xcrun --find clang++ 2>/dev/null || echo /usr/bin/clang++)"
    # cc-rs invokes the host compiler WITHOUT an -isysroot, and with NDK_BIN on PATH the usual xcrun-driven SDK discovery doesn't happen — so host C deps (blake3's NEON path) failed on a missing assert.h. Name the SDK explicitly for the host-target flags only; the Android target's flags are untouched.
    _macos_sdk="$(xcrun --sdk macosx --show-sdk-path 2>/dev/null)"
    if [ -n "$_macos_sdk" ]; then
        export SDKROOT="$_macos_sdk"
        export CFLAGS_aarch64_apple_darwin="-isysroot $_macos_sdk"
        export CXXFLAGS_aarch64_apple_darwin="-isysroot $_macos_sdk"
    fi
fi
