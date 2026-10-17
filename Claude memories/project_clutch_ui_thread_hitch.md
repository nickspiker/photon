---
name: project_clutch_ui_thread_hitch
description: "FIXED 2026-08-15 @ c48b0e1 — CLUTCH weave looked stuck: 8 PQ decaps ran inline on the UI thread (1.2 s freeze) and every duplicate KEM/Complete re-ran them; decap is now the 4th background job stage"
metadata:
  type: project
---

2026-07-08 (Samsung weave): `PERF: check_clutch_ceremonies took 1205ms (UI thread)` froze the screen on "weaving the chain", and each duplicate ClutchComplete/KEM re-ran the ceremony, feeding the retransmits that caused it.
Fixed c48b0e1: the PQ decapsulations are the FOURTH background job stage (HQC-prefix CAS on drain, one consolidated drain); duplicate KEM responses drop before spending anything; the chains save rides the durable coalescing writer and the ClutchComplete proof fires post-durability (`ChainsPostDurable::CeremonyProof`).

**How to apply:** ceremony crypto never runs on the UI thread; a duplicate must short-circuit before any expensive work. Tell: `PERF: check_clutch_*` lines in a device log. Related: [[ui-thread-snapshot-cas]].
