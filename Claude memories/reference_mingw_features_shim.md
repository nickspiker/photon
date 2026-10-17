---
name: reference_mingw_features_shim
description: "x86_64-windows build: pqcrypto-mlkem's #include <features.h> needs the cross-libs/gcc-noglibc-shim/features.h shim (wired via .cargo/config.toml [env]); cc-rs hides C-compile errors as cargo:warning="
metadata:
  type: reference
---

`x86_64-pc-windows-gnu` fails at `pqcrypto-mlkem`: PQClean's `compat.h` does `#include <features.h>` under `__GNUC__ && !__clang__` just to call `__GNUC_PREREQ` — MinGW-GCC has no glibc `features.h`.

**Fix (2026-08-14):** a minimal `features.h` defining only `__GNUC_PREREQ`, now at `cross-libs/gcc-noglibc-shim/features.h`, on the include path via `.cargo/config.toml` `[env] CFLAGS_x86_64_pc_windows_gnu = "-I/mnt/Harbor/Code/photon/cross-libs/gcc-noglibc-shim"` — automatic for dev and deploy. aarch64-windows uses clang (llvm-mingw) and needs no shim.
Lesson from the retired Redox target: use `#include_next` when the target HAS the header but it is incomplete; a plain stub only when the header is absent.

It "built fine before" because an earlier release linked a cargo-cached C object; the first clean C recompile surfaced it.

**LOAD-BEARING LESSON — cc-rs hides C errors:** a failed C compile in a build.rs is re-emitted as `cargo:warning=` lines and cargo shows only "failed to run custom build command for X". Grep the `cargo:warning=` lines for `fatal error`.
Related: [[reference_windows_arm64_toolchain]], [[reference_aarch64_cross_libs]].
