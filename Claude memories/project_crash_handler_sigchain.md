---
name: project_crash_handler_sigchain
description: Android native-fault handler LESSONS 2026-09-10 — arm once the log dir is known; libsigchain ignores SA_RESETHAND; Kotlin decodes tombstones; unstripped .so archived per build; JNI entry points from worker threads must post to the main looper
metadata:
  type: project
---

crash_native.rs on Android (shipped 2026-09-10, v0.89.18–.23):
- Arm from `NetworkContext::new` via `arm_crash_reporting()` — JNI_OnLoad runs before the log dir exists, so the handler had never installed.
- ART's libsigchain does NOT honour SA_RESETHAND: `raise(sig)` re-entered the handler 51,011 times and the process spun at 100% CPU. Now: re-entry latch → `_exit`, restore the saved disposition, unblock, raise, `_exit` fallback.
- `crash_flush_pending()` writes the soft log batch from the handler; the batch also writes thru at 30 s of age (an ANR "close app" is SIGKILL, no edge).
- Tombstone.kt decodes ApplicationExitInfo traces (signal, abort message, frames with rel_pc + build id). android.sh archives each build's unstripped .so under /mnt/Harbor/Code/photon-symbols/android-arm64; symbolize with `llvm-symbolizer --obj=<so> 0x<rel_pc>` (NDK binary under ~/android-sdk). A rebuild at the same commit does NOT reproduce addresses.
- A SIGBUS right after an APK install is the package being replaced under the old process, not a bug. `adb bugreport` samples a spinning non-debuggable app's stacks.

**2026-09-12 (v0.92.8) Scudo double free:** a network-thread `request_service_tick` ran `nativeServiceTick` INLINE on the caller thread → advance_protocol raced the UI thread. Fix: Kotlin posts the tick to the main looper (coalesced).

**Why:** three days of "force close constantly" reports with nothing actionable in the log.
**How to apply:** for any Android crash, find `tombstone:` lines and symbolize against the archived .so of that exact version. Nothing but the main thread touches PhotonApp: JNI entry points invoked from Rust worker threads post, never call. Related: [[project_android_hang_nag]].
