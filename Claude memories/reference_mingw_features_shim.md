---
name: reference_mingw_features_shim
description: x86_64-windows build breaks on pqcrypto-mlkem's #include <features.h> (MinGW has no such glibc header); fixed with a vendored features.h shim wired via .cargo/config.toml [env]. Also: cc-rs hides C-compile errors as cargo:warning=.
metadata:
  type: reference
---

The `x86_64-pc-windows-gnu` build fails at `pqcrypto-mlkem 0.1.1`: its vendored PQClean `compat.h` does `#include <features.h>` inside `#if defined(__GNUC__) && !defined(__clang__)`, purely to call `__GNUC_PREREQ(7,1)`. That assumes GCC implies glibc — false for MinGW-GCC, and Fedora's mingw ships no `features.h` (it's a glibc header). FIXED @ 4809917 2026-08-14.

**The fix:** a minimal `features.h` (defines only `__GNUC_PREREQ`) vendored at `cross-libs/mingw-shim/features.h`, put on the C include path via a `[env]` table in `.cargo/config.toml`:
```
[env]
CFLAGS_x86_64_pc_windows_gnu = "-I/mnt/Octopus/Code/photon/cross-libs/mingw-shim"
```
Automatic for dev AND deploy (not deploy-only). The aarch64-windows build uses clang (llvm-mingw), which takes compat.h's `__clang__` branch and never includes `features.h` — so ONLY x86_64-gnu needs the shim.

**Why it "built fine before then broke with no code change":** the failure is a standalone crate-vs-toolchain bug (reproduced building bare `pqcrypto-mlkem 0.1.1` for x86_64-windows-gnu with zero photon involvement). The July-26 v51 release almost certainly linked a cargo-CACHED C object from before the mingw update; the first clean C recompile (this deploy, after cache churn) surfaced it. NOT caused by the ARM64 work or the fgtw/fluor bump — verified: identical crate version before/after, fails in isolation.

**LOAD-BEARING LESSON — cc-rs hides C errors:** when a C compile invoked by a crate's build.rs (via the `cc` crate) fails, cc-rs re-emits the compiler's stderr as `cargo:warning=` lines and the build script just exits non-zero. So cargo reports only an opaque "failed to run custom build command for X" and the REAL cause (`fatal error: features.h: No such file`) is demoted to warning noise. When a `-sys`/C crate build fails cryptically, grep the `cargo:warning=` lines for `fatal error`. Same disease as the deploy.sh silent-failure — the load-bearing error disguised as noise; the deploy's own `DEPLOY FAILED at line N` trap (commit c4edaed) was MORE useful than cargo's output here. Related: [[reference_windows_arm64_toolchain]], [[reference_aarch64_cross_libs]].

## Redox: same root cause, DIFFERENT fix (2026-08-14, @c9277d3)

The first full deploy.sh since pqcrypto-mlkem was added (2026-08-01) also broke REDOX — dev publishes (mac/android only) never build Redox, so it hid until a full release. TWO Redox breaks:

1. **features.h / __GNUC_PREREQ** — same compat.h line as Windows, BUT the fix differs. MinGW has NO features.h (plain stub suffices). Redox HAS a musl-derived relibc features.h that its OWN system headers (sys/time.h, stdlib.h) depend on for __noreturn/__deprecated — so a REPLACEMENT stub broke those headers (`expected ';' before 'int'`, `unknown type name '__deprecated'`). Redox's features.h just lacks __GNUC_PREREQ (musl doesn't define it). FIX: a SEPARATE shim at cross-libs/redox-features-shim/features.h that does `#include_next <features.h>` (chains the real one, keeping all relibc macros) then ADDS __GNUC_PREREQ. Wired via CFLAGS_x86_64_unknown_redox. Lesson: `#include_next` when the target HAS the header but it's incomplete; plain stub only when the header is entirely ABSENT.

2. **libc::forkpty absent on Redox** — bridge's PTY host (network/bridge.rs) calls libc::forkpty; Redox has no PTY. Redox is cfg(unix)=true so it slipped the existing `all(unix, not(android))` gate. FIX: added `not(target_os = "redox")` to all 9 bridge gate sites (2 in bridge.rs, 7 in photon_app.rs) — one uniform predicate, replace_all-safe. Bridge is desktop-unix-only by design. This was the forkpty issue flagged way back in [[project_bridge]] — finally hit + fixed in a real build.
