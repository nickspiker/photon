---
name: reference_windows_arm64_toolchain
description: "Windows-on-ARM (aarch64-pc-windows-gnullvm) build recipe — llvm-mingw sysroot at /mnt/Harbor/Code/llvm-mingw, wired into deploy.sh as a REQUIRED target"
metadata:
  type: reference
---

Photon builds native Windows-on-ARM via `aarch64-pc-windows-gnullvm` (proven 2026-08-14: valid ARM64 PE, signs clean; never run on real hardware).

**Toolchain (build-box dependency, not in the repo):** `/mnt/Harbor/Code/llvm-mingw` = mstorsjo's prebuilt llvm-mingw (ucrt, x86_64 host); re-fetch from github.com/mstorsjo/llvm-mingw/releases if the box is rebuilt. gnullvm needs no MSVC/xwin; the `aarch64-w64-mingw32-clang` wrapper is both the C compiler and the linker (raw `lld-link` can't find the import libs).

```
PATH="/mnt/Harbor/Code/llvm-mingw/bin:$PATH" \
CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER="/mnt/Harbor/Code/llvm-mingw/bin/aarch64-w64-mingw32-clang" \
cargo build --release --target aarch64-pc-windows-gnullvm
```

- `build.rs` uses `llvm-rc` for the icon on aarch64 (the only source change). A rustls→ring pin was tried and reverted as inert — don't re-add it.
- deploy.sh ABORTS if the toolchain is absent (Nick 2026-08-14: "a deploy ships every platform or ships none").
- `install-release.ps1` picks the arm64 asset on ARM64 and carries per-arch expected hashes.
Related: [[reference_mingw_features_shim]], [[feedback_build_dev_script]].
