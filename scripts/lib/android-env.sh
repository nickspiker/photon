# Sourced, not executed. Sets up the Android NDK + cross-toolchain env for an
# aarch64-linux-android cargo build. Detects ANDROID_HOME, then derives the NDK from it
# (was hardcoded + drifted across the old scripts), plus the ring-crate clang symlinks and
# the host build flags that build.rs needs.

if [ -d "/home/nick/android-sdk/build-tools" ]; then
    export ANDROID_HOME=/home/nick/android-sdk
elif [ -d "/home/nick/Android/Sdk/build-tools" ]; then
    export ANDROID_HOME=/home/nick/Android/Sdk
else
    echo "ERROR: Cannot find Android SDK (looked in /home/nick/android-sdk and /home/nick/Android/Sdk)"
    exit 1
fi

export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/25.2.9519653"
NDK_BIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
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
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="clang"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"
export CC="clang"
export CXX="clang++"
