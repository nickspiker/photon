---
name: project_background_noise
description: Background noise redesign 2026-11-21: one clean full-screen field (no split at panes) and a true 16-bit walk generated where drawn are BUILT; next the pane-composite model (panes pre-rendered RGBA composited onto it)
metadata:
  type: project
---

Owner, 2026-11-21, on the contacts screen's split background (top half still, bottom half rolling; a ferros pinned-search change, 8e0a8155) and Settings' rail/content split: "that galois should be clean for the whole screen"; "there should be RGBA panes we composite onto said background that are already pre-rendered so it's only one RAM hit"; "I think what I said earlier got misunderstood so we'll need to work it thru step by step." (2026-11-08 was recorded as "keep a background buffer that you generate once for Photon and just rotate it vertically" -> fluor NoiseField, a cached 16 MB / 32 MB field.)
Decided 2026-11-21: the generator becomes a TRUE 16-bit walk (finer steps than the 8-bit one), its output taken straight as G2t16 with no 8->16 widening ("true sixteen and just assume it's g2t so it's a straight thru, it'll make the speckles pop a smidge anyway so that's good").
**Why:** the split shows seams; the cached field is a RAM read per pixel on the Pro (field far larger than its cache); widening costs a second pass.
**How to apply:** step by step with the owner, no big-bang build.
- Step 1 (2026-11-21): measured generate vs cache on the desktop at 1344x2992 (fluor/tests/noise_bench_tmp.rs, temporary): one core, cache 2.7x faster (field fits the Ryzen's 64 MB L3); all cores at 16 bits, generating 2.1x faster. The Pro's numbers were requested from the ferros session (one A715, all A715s, one A510).
- Step 2 (2026-11-21, BUILT): generated wins on every core (Pro: one A715 10.73 vs 11.65 ms cached, four A715s 5.68 vs 7.78, one A510 19.37 vs 20.79; table in fluor src/paint/noise.rs). fluor `noise_under` (NoiseField gone): 16-bit lanes, step = two 9-bit draws a lane (bit 8 = the old 1-code step, low byte = grain), 4 rows interleaved a worker, store into empty else under; 8-bit = top bytes. Photon: one offset for the whole screen (`noise_scroll` = main pane; Settings = content, rail moves nothing), axis at centre, `noise_pin` an isize; `noise_drawn` = what the screen holds, damage_rect makes a moved-noise frame whole unless a memmove carries it, and a memmove frame redraws everything outside the pane (`around_pane`).
- Next: the pane-composite model in fluor (panes pre-rendered RGBA, scroll = a pane's read offset, background never moves with content, no memmove/noise roll/ring).
Related: [[project_scroll_memmove]], [[feedback_smoothness_for_humans]].
