---
name: reference-fluor-test-features
description: fluor's chrome tests and the golden_chrome scene are cfg(feature = "icon"); check fluor with cargo test --features rayon,icon or they never run
metadata:
  type: reference
---

fluor's `host::chrome_widget` (DefaultChrome) and the `golden_chrome` scene compile only with `text` + `icon`, and `icon` is not a default feature.
A plain `cargo test` in fluor runs 168 tests and silently skips them; `cargo test --features rayon,icon` runs 182.

**Why:** fluor a5751cb (2026-11-07) intentionally moved the chrome's hairline weight and re-recorded every golden except `golden_chrome`, because that one was gated out of the run; it sat failing unnoticed until 2026-11-17 (re-recorded at ebfc14c after a bisect).
**How to apply:** when reviewing or merging any fluor change, run `cargo test --features rayon,icon`; a golden that moves needs its cause found (bisect) before it is re-recorded, never re-recorded blind.
