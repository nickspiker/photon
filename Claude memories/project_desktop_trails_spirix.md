---
name: project_desktop_trails_spirix
description: "2026-09-18: the DESKTOP's path deps can trail their remotes (spirix 5932952 never pulled) — a dozenal test sat red 3 days and v99 shipped ASCII digits; fetch + ff-only every path sibling before building"
metadata:
  type: project
---

photon 0bbca9e0 (2026-09-15) made `fmt_mag` rely on spirix's `{:#}` emitting digits into photon's private-use glyph block; that spirix half (5932952) was pushed to `origin/spiral` from the Mac and never pulled on the desktop.
Result: `base_kat::every_count_is_a_magnitude_in_dozenal` red for three days, carried as "known pre-existing" until Nick asked "It's known?"; deploy v99 and Android dev builds v0.98–0.99 rendered dozenal counts as ASCII digits.
Fixed by `git -C ../spirix merge --ff-only origin/spiral` (tohu trailed by one commit too).

**Why:** deploy.sh's SEAM gate refuses a DIRTY sibling but never compares a sibling to its remote, so a clean-but-stale path dep passes silently.
**How to apply:** whichever machine builds fetches every path sibling (spirix, vsf, fluor, opsin, limbus, tohu, fgtw, ihi) and fast-forwards any that trail; a red test is never "known" — find its commit and check whether a sibling remote carries the other half. Mirror case: [[macbook-trails-remote]]. Open: teach the SEAM gate to compare each sibling's HEAD to `origin/<branch>`. Related: [[feedback_test_discipline]].
