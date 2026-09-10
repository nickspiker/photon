---
name: project_crash_handler_sigchain
description: Android native-fault handler LESSONS 2026-09-10 — arms only once the log dir is known; libsigchain ignores SA_RESETHAND (re-raise recursed 51k times); Kotlin decodes tombstones; unstripped .so archived per build
metadata: 
  node_type: memory
  type: project
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-10T21:25:24.257Z
---

crash_native.rs on Android (all shipped 2026-09-10, v0.89.18–.23):
- Arm from `NetworkContext::new` via `arm_crash_reporting()` — JNI_OnLoad runs before the log dir exists, so the handler had never installed and every native crash was a bare tombstone.
- ART's libsigchain wraps app handlers and does NOT honour SA_RESETHAND: `raise(sig)` at the end of the handler came straight back in, 51,011 sidecar lines, and the process lived on spinning at 100% CPU. Now: re-entry latch → `_exit`, restore the saved previous disposition, unblock, raise, `_exit` fallback; the prior-crash report is capped at 48 lines.
- `crash_flush_pending()` writes the soft log batch from the handler; the batch also writes thru at 30 s of age (an ANR "close app" is SIGKILL, no edge).
- Tombstone.kt decodes ApplicationExitInfo's binary trace (signal, abort message, crashing thread frames with rel_pc + build id); android.sh keeps each build's unstripped .so under /mnt/Harbor/Code/photon-symbols/android-arm64 (profile strip overridden with CARGO_PROFILE_RELEASE_STRIP=none + line tables). Symbolize: `llvm-symbolizer --obj=<so> 0x<rel_pc>` (NDK 25 binary under ~/android-sdk). A rebuild at the same commit does NOT reproduce addresses — only the archived .so matches.
- A SIGBUS in the old process right after an APK install is the package being replaced under it, not a bug.
- `adb bugreport` dumps every process's thread stacks (FS/data/anr/* and the main txt) — the way to sample a spinning non-debuggable app.

**Why:** three days of "force close constantly" reports with nothing actionable in the log.
**How to apply:** for any Android crash, pull the log and look for `tombstone:` lines, then symbolize against the archived .so of that exact version. Related: [[project_android_hang_nag]].
