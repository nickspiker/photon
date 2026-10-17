---
name: project-clutch-token-asymmetry
description: "\"unknown conversation_token\" / proof-mismatch stalls = competing ceremony instances (fleet-sync.md §4.2); one-ceremony discipline 77390ec, re-send token fix b6f18b9, offer-device binding b318b13 (2026-07-24)"
metadata:
  type: project
---

Three root causes found 2026-07-24 (David, William, Michael, Jennifer, Sarah ↔ Nick), all fixed:
1. **Competing instances (77390ec):** parking only gated the keygen queue, so devices kept in-flight rounds alive after another claimed → friends bound to one instance and dropped the other's ClutchComplete forever. Fix = one-ceremony discipline: `Contact::discard_clutch_round()` canonical teardown; takeover needs a real presence VERDICT (`presence_probed`), never boot-default offline; owner_woven parks forever; all seven keep-alive paths gated; a mid-ceremony new-keys offer is adopted wholesale. (Ownership is now claim-first: [[project_ceremony_claim_first]].)
2. **Re-send paths used the RAW identity seed (b6f18b9):** first sends were right, every retransmit rode a garbage token. Lesson: grep ALL `derive_conversation_token` call sites when hunting token asymmetry — the pid seam had one more unmigrated site than believed.
3. **Proof mismatch on identical builds (b318b13):** eggs folded "their device" from the pinned contact identity, which pongs re-elect; completion now binds the OFFER pair (`PartySlot.offer_device` = verified signer of the stored offer).

Also: an ungated recovery path re-sent a 548 KB offer 151× over the relay (cost). Deferred: a persistent token mismatch should surface as "remove & re-add" instead of retransmitting forever.
Related: [[project_clutch_offer_deadlock]], [[reference_log_pull]].
