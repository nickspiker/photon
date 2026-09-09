---
name: feedback_sed_address_guard
description: never run a sed address op (Na / Ni / Ns) with a line number pulled from grep unless the variable is verified non-empty — an empty address applies to EVERY line (2026-09-09: lib.rs got the same map line stamped 1271 times, committed AND pushed before it was caught)
metadata:
  type: feedback
---

An empty `$n` in `sed -i "${n}a\..."` is `sed -i "a\..."`: the append runs on every line of the file. The same for `i` and `s`.

**Why:** 2026-09-09 the source-map append for audio_aaudio.rs anchored on a grep that matched nothing; the line was stamped after all 1271 lines of src/lib.rs, it still compiled (all comments), and it went out in commit a00518e before the next check saw it.

**How to apply:** `[ -n "$n" ] || { echo "anchor missing"; exit 1; }` before any addressed sed, or use the Edit tool for anchored insertions; after any scripted edit, sanity-check with `wc -l` / `grep -c` before building or committing.
