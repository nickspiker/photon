---
name: project_clutch_ui_thread_hitch
description: "CLUTCH weave feels \"stuck/forever\" — ceremony runs on UI thread (1.2s freeze) + ClutchComplete retransmit storm"
metadata: 
  node_type: memory
  type: project
  originSessionId: 214fbb36-8068-4595-9dbb-870b43abd44e
---

The chain-weave can look stuck on "weaving the chain" for many seconds even though the protocol is grinding forward — two pre-existing perf issues in the CLUTCH path (NOT a regression; surfaced 2026-07-08 watching a fresh Samsung↔Nick weave closely).

Evidence from the on-device VSF log (the test phone, SM-N976V):
- `PERF: check_clutch_ceremonies took 1205ms (UI thread)` — the ceremony blocks the render thread ~1.2s, so the UI hard-freezes on "weaving the chain" with no redraw.
- ClutchComplete received **8 times** over ~6s (Nick retransmitting) before the proof finally verified — the ~6s stall before `Early proof verified` is where the "forever" went. Once it started, weave→woven was only 1.5s.

Two suspects: (1) the expensive ceremony work (likely spaghettify / memory-hard derivation) runs on the UI thread — should be off-thread like the network worker, or at least not block rendering; (2) each incoming duplicate ClutchComplete may re-run the ceremony, compounding the hitch and never quieting fast enough, so the peer keeps resending. Related: [[project_clutch_completion_rebroadcast]], [[project_clutch_offer_deadlock]].

**Why:** cosmetic-looking but real — a 1.2s UI-thread freeze is a jank bug, and the retransmit storm wastes radio + compounds the freeze.
**How to apply:** when returning to CLUTCH perf, move the ceremony derivation off the UI thread and dedup/short-circuit repeated ClutchComplete so a duplicate doesn't re-run the full ceremony. Parked 2026-07-08 in favour of shipping the UI/notification/chirp batch.

## FIXED 2026-08-15 @ c48b0e1 — decap off-thread + duplicate short-circuit + proof post-durability

The 8 PQ decapsulations (three inline drain arms — the 2026-08-08 offload's "deliberately inline" residue) are now the FOURTH background job stage: HQC-prefix CAS on drain (same staleness identity as the wire), one consolidated drain replaces the three half-copies (store secrets, offer backfill, encap trigger, completion check).
Duplicate KEM responses now drop before spending anything — every retransmit used to re-run all 8 decapsulations inline, compounding the freeze that caused the retransmits.
Ceremony chains save rides the durable coalescing writer; the ClutchComplete proof is a gated signal (ChainsPostDurable::CeremonyProof) firing post-durability.
Remaining inline (deliberate, ms-scale, once per ceremony): the 32-byte fanout pair store and save_contact.
E2E on a real phone weave pending — watch for "PERF: check_clutch_*" lines gone from the device log.
