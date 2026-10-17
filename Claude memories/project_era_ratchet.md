---
name: project_era_ratchet
description: ERA RATCHET (re-key without wiping) stages 1-4 SHIPPED 2026-09-08 — eras first-class, completion supersedes + retires, era tags on the wire, hybrid light ratchet, woven CLUTCH on shrink; NEXT stage 5 consent/banner, stage 6 timers→edges + soak
metadata:
  type: project
---

Nick's mandate 2026-09-08: a re-clutch must never wipe the live chain (Emma: "cannot send — no friendship chain", compose bar gone).
Next era = KDF(old lane_root ‖ old history_key ‖ fresh secret ‖ transcript): continuity from the old root, confidentiality ONLY from the fresh secret.
Decisions: shrink (revoke/release) → a FULL CLUTCH woven with the old roots for every friendship; a from-scratch CLUTCH on an existing conversation → consent (knock) + banner, old chain live until completion; light ratchet KEM = ML-KEM-1024 + X25519 + HQC-256; cadence every 256 peer rows (an edge, not a timer).

**Shipped (2026-09-08):**
1. Era model on FriendshipChains (`era_index`, `era_lineage`, `lane_eras`, `retired`, `pending`); completion calls `supersede_with` (old era RETIRED read-only, dropped after RETIRED_ERA_GRACE_ROWS = 64 current-era frames).
2. Chat-frame `era` tag + pong era row; `era_pull`; src/ui/photon_app/era.rs `friendship_repair` (pure, table-tested) + `repair_dispatch`.
3. src/crypto/era.rs light ratchet (EraSignal Init/Resp/Nudge hidden rows).
4. Woven full CLUTCH (`from_clutch_woven`, era-prior claim in the offer); shrink edges arm `era_weave_due`.

**Rules the field left:**
- Lanes are stamped with the era root's TAG, never its index (a fresh ceremony is index 0 again).
- A round completes ONCE (`Contact::ceremony_completion_hold`); `supersede_with` is a no-op on the same root.
- e6-stamped numerals need the explicit e6 reader — `get_value::<i64>` read 0 and two live eras refused each other forever ([[feedback_vsf_readers_width_agnostic]]).
- Ceremony ownership is claim-first now ([[project_ceremony_claim_first]]).

**NEXT:** stage 5 consent/banner/pinned_genesis, stage 6 timers→edges + soak. Field tells: "ERA: …" lines, "Init sent", "Resp sent", "cut over …", supersede index N+1. Related: [[project_braid_v2]], [[project_clutch_completion_rebroadcast]].
