---
name: reference_windows_arm64_toolchain
description: "Windows-on-ARM (aarch64-pc-windows-gnullvm) target build recipe — llvm-mingw sysroot at /mnt/Harbor/Code/llvm-mingw, wired into deploy.sh; proven to build a valid ARM64 PE."
metadata: 
  node_type: memory
  type: reference
  originSessionId: b192f764-fe07-4644-91fb-09156e2e7e05
---

Photon builds **native Windows-on-ARM** (Snapdragon X / Copilot+ PCs, no x86 emulation) via the `aarch64-pc-windows-gnullvm` Rust target. Proven 2026-08-14: a real 33M `PE32+ ... ARM64` (machine 0xAA64) binary builds + signs clean.

**Toolchain (build-box dependency, NOT in the repo — a sibling dir like osxcross/redox):**
`/mnt/Harbor/Code/llvm-mingw` = mstorsjo's prebuilt llvm-mingw (tag 20260616, ucrt, ubuntu-x86_64 host). Bundles clang + lld + llvm-rc + a complete `aarch64-w64-mingw32` ucrt sysroot. Re-fetch from github.com/mstorsjo/llvm-mingw/releases if the box is rebuilt.

**Why gnullvm not msvc:** gnullvm is the LLVM-MinGW target — needs NO MSVC/xwin. The C deps (ring, pqcrypto-internals, aws-lc-sys) compile against the llvm-mingw sysroot's headers/CRT. The `aarch64-w64-mingw32-clang` wrapper (on PATH) is BOTH the C compiler cc-rs auto-detects AND the linker (it knows its own import lib dir — raw `lld-link` fails with "unable to find library -lbcrypt" etc. because it doesn't).

**The three-line build invocation (what deploy.sh does):**
```
PATH="/mnt/Harbor/Code/llvm-mingw/bin:$PATH" \
CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER="/mnt/Harbor/Code/llvm-mingw/bin/aarch64-w64-mingw32-clang" \
cargo build --release --target aarch64-pc-windows-gnullvm
```

**Code change required (shipped):** `build.rs` uses `llvm-rc` (not mingw windres) for the icon on aarch64 — the ONLY source change. A rustls→ring pin was tried and REVERTED as inert: the sysroot compiles aws-lc's C fine, so no crypto-backend change was needed (don't re-add it).

**deploy.sh wiring (shipped):** builds+signs the arm64 target after x86_64 Windows. ARM64 is a REQUIRED target like every other platform — if `/mnt/Harbor/Code/llvm-mingw/bin/aarch64-w64-mingw32-clang` is absent the deploy ABORTS with an error (`exit 1`), no silent skip (Nick's call 2026-08-14: "a deploy ships every platform or ships none" — matches the no-silent-failure stance behind the Redox/set-e fix). A build FAILURE also hard-fails via snap_cargo's ERR trap. Uploads `photon-messenger-windows-arm64-release.exe` to R2 + GitHub mirror, adds a `Windows arm64` manifest artefact row (all unconditional). `photon-signature-signer` signs the arm64 .exe and writes its .sha256 (it detects any .exe as a Windows binary).

**installer (install-release.ps1, shipped):** already detected `$env:PROCESSOR_ARCHITECTURE ARM64` but always downloaded x64; now picks `-windows-arm64-release.exe` for ARM64. Holds per-arch `$expectedHashX64`/`$expectedHashArm64` (deploy.sh patches both; arm64 stays a zero placeholder for releases built without the toolchain, so an ARM64 box correctly refuses a wrong/absent asset). NOTE: NOT tested on a real ARM64 Windows machine — the .exe is a valid PE but end-to-end install + run is unverified (no hardware). See [[reference_aarch64_cross_libs]] (the analogous aarch64-LINUX cross-lib vendoring pattern) and [[feedback_build_dev_script]].
