---
name: project_desktop_trails_spirix
description: "2026-09-18 the DESKTOP's path deps can trail their remotes too (spirix glyph-block formatter 5932952 was on origin/spiral, never pulled here) — photon's dozenal test went red for three days, and v99 + every Android dev build from this machine rendered dozenal magnitudes as ASCII digits; fetch + ff-only every path sibling before a build here"
metadata: 
  node_type: memory
  type: project
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-18T04:41:10.871Z
---

**What happened:** photon commit 0bbca9e0 (2026-09-15, "dozenal is exponential, period") made `fmt_mag` rely on spirix's `{:#}` emitting digits into photon's private-use glyph block (`0x10 + digit`).
That spirix change (5932952, "formatting: `#` emits digits in the private-use block", same day) was pushed to `origin/spiral` from the Mac and never pulled on the desktop, whose spirix sat at add92ca (2026-09-08).
Result on the desktop: `base_kat::every_count_is_a_magnitude_in_dozenal` red ("one is Zil" got ASCII "0"), carried as "known pre-existing" thru several batches until Nick asked "It's known?"; worse, deploy **v99** (2026-09-18) and every Android dev build v0.98.x–v0.99.x built here rendered every dozenal count as ASCII digits with A/B for ten/eleven instead of Oxanium glyphs.
Fixed by `git -C ../spirix merge --ff-only origin/spiral` (and tohu, which trailed by one additive commit a0b6b0a); suite 390/0 green after.

**Why:** deploy.sh's SEAM gate refuses on a DIRTY sibling but never compares a sibling to its remote, so a clean-but-stale path dep passes silently and the test suite is the only tell — which a "known red" habit defeats.
[[macbook-trails-remote]] is the mirror case (the Mac trailing); the rule is symmetric: whichever machine builds must fast-forward ALL path siblings first.

**How to apply:** before dev.sh / deploy.sh / dev-android.sh on this desktop, `git fetch` every path sibling (spirix, vsf, fluor, opsin, limbus, tohu, fgtw, ihi) and ff-only any that trail; a red test is never "known" — find the commit that introduced it and check whether a sibling remote carries its other half.
Open: teach the SEAM gate to compare each sibling's HEAD to `origin/<branch>` and refuse (or warn) when it trails; toka has 3 unpushed local commits (Nick's, Aug 2026) and opsin's `src/view.rs` is dirty (Nick's in-progress) — neither photon-blocking.
Related: [[project_numeral_forms]], [[feedback_test_discipline]].
