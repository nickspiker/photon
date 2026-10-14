---
name: feedback-test-discipline
description: Publish IS the compile gate; ONE full cargo test per batch before push; never --lib-only greens (it skips tests/ — call_media_loop sat broken 3 days)
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-13T22:09:09.250Z
---

Nick, 2026-09-13, stopwatch in hand: a cargo test build costs minutes of CPU even erroring out, "when a simple publish will already tell you".

**Why:** each cargo test builds a separate test-profile artifact set on top of the publish's target build — pure duplication as a compile check. Worse: `cargo test --lib` silently skips `tests/` integration targets; `tests/call_media_loop.rs` was broken for 3 days (derive_call_secret v2) under repeated `--lib` "green" claims.

**How to apply:** edit → publish (`./scripts/publish/dev-android.sh` or dev.sh — the compile gate) → one FULL `cargo test` per batch before push, only when tests/tested logic changed. Never claim green from a subset. Never dev.sh check + cargo test + publish in one iteration. Related: [[feedback_build_dev_script]].

**2026-09-29 repeat offence (Nick: "full test, then a full build, then a publish which invokes another full build?"):** I ran `cargo test` + `scripts/android/build.sh` + `dev-android.sh` per batch. That's three builds for one answer. `android/build.sh` before a publish is PURE duplication: the publish builds the same APK. When a publish is asked for, the publish IS the Android compile check. Don't run build.sh at all unless no publish follows.

**2026-10-13 (Nick: "when you do a build… then when you run an android deploy for dev it literally builds again, no?"):** NEVER run a standalone `android_build` (or any platform build) before a dev publish of the same tree — the publish pins PHOTON_STAMP_COMMIT + the version into the binary, which forces the photon crate to recompile anyway, so build-then-publish ≈ two full builds. The sequence is: ONE `cargo test` (host target, a different artifact — not a duplicate), commit, then the publish as the ONLY phone/Mac build. `cargo check --target …` (no codegen) is fine for a platform the test can't reach, when nothing is being published. A killed mid-configure Android build leaves a stale audiopus_sys CMake cache that resets the install prefix to /usr/local on the next run — remove target/aarch64-linux-android/release/build/audiopus_sys-*/ before rerunning.

Observed 2026-10-13: the FIRST `cargo check --target aarch64-linux-android` after a host `cargo test` fails in audiopus_sys's build script (CMake reconfigure); the immediate rerun passes. Repeatable, unconvicted; the release-profile publish dir has not shown it. If a publish ever fails there, rerun once before digging.
