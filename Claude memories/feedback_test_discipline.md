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
