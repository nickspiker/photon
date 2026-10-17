---
name: feedback-test-discipline
description: "Publish IS the compile gate; ONE full cargo test per batch before push; never --lib-only greens (it skips tests/); never build before a publish of the same tree"
metadata:
  type: feedback
---

Nick, 2026-09-13, stopwatch in hand: a cargo test build costs minutes of CPU even erroring out, "when a simple publish will already tell you".

**Why:** each cargo test builds a separate artifact set — pure duplication as a compile check. `cargo test --lib` silently skips `tests/` (`tests/call_media_loop.rs` sat broken 3 days under `--lib` "green" claims). Repeat offences 2026-09-29 ("full test, then a full build, then a publish which invokes another full build?") and 2026-10-13.

**How to apply:**
- Sequence: edit → ONE full `cargo test` per batch (only when tests/tested logic changed; host target) → commit → the publish as the ONLY phone/Mac build. Never claim green from a subset.
- Never run a standalone platform build (`scripts/android/build.sh`, `android_build`) before a dev publish of the same tree — the publish pins PHOTON_STAMP_COMMIT + version, forcing a recompile anyway.
- `cargo check --target …` (no codegen) is fine for a platform the test can't reach when nothing is being published.
- Gotchas: a killed mid-configure Android build leaves a stale audiopus_sys CMake cache (install prefix → /usr/local) — remove target/aarch64-linux-android/release/build/audiopus_sys-*/ before rerunning. The first `cargo check --target aarch64-linux-android` after a host `cargo test` fails in audiopus_sys's build script; the immediate rerun passes (unconvicted) — rerun once before digging.
Related: [[feedback_build_dev_script]], [[feedback-one-build-per-check]].
