---
name: project_clutch_completion_rebroadcast
description: CLUTCH completion + re-key churn — chain-weave probe seals completion; same-era replication keeps an open round Pending; sibling proof mismatch convicted 2026-09-18 (decap drain skipped the sibling party-id seam); ghost 1be949c1 retire open
metadata:
  type: project
---

**Completion handshake (2026-07-05):** proofs matched but the initiator re-sent forever, because the ceremony was decoupled from the data plane. Fix: a hidden chain-weave probe (`CHAIN_PROBE_MARKER`) each way on Complete; `Contact.chain_woven` seals once both directions are proven (any real message also seals), zeroing the proof resends.

**Rules from the 2026-09-10 field (Esme/Emma ↔ Nick):**
- era_pull carries the asker's era TAG; a sibling holding the same era answers miss (else it served the asker its own era forever) — d0ace9c.
- Same-era chain-sync replication landing mid-round leaves the round Pending; only an era move discards it (f001b85). Before, the round died and the whole fleet fell an era behind the friend.
- Every device must run the build: an old-build device that wins the owner election holds forever.

**Sibling churn generator CONVICTED + FIXED 2026-09-18:** `check_clutch_kem_decaps` (ceremony.rs) used the identity party id for every row, skipping the sibling seam (`sibling_party_id(device_pubkey)` / `our_party_id`) that keygen, encap and completion apply → a second encap overwrote the first secrets → deterministic PROOF MISMATCH on every sibling round. Fix: the decap drain takes `self.our_party_id(&contact)`.
Tells of the bug: two "Sent KEM response" per round with different HQC prefixes, "Spawning KEM encap … after decap", "PROOF MISMATCH (same round)", "zombie round expired" at next launch.

**How to apply:** never reload/deploy the ceremony-owning desktop while a round is open ([[feedback_reload_mid_ceremony]]). A seam (sibling vs friend party id) must be applied at EVERY drain — grep them all.
**OPEN:** the ghost 1be949c1 row (Pending forever, offers every launch, takes keygen pickup) needs the retire flow. Related: [[project_clutch_offer_deadlock]], [[project_era_ratchet]].
