---
name: project-braid-v2
description: "2026-09-27 BUILT (unshipped until Nick publishes): braid v2 weaves S = spaghettify(domain ‖ strand_time ‖ len ‖ text) per strand, agreed per ERA at CLUTCH (option 2), migrated by heavy weave once every friend device pongs v2; prerequisite for real delete/truncate; field verify pending"
metadata:
  node_type: memory
  type: project
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-28T03:06:18.568Z
---

Nick 2026-09-27: the weave should include a spaghettification of each woven message PLUS its time so every strand is unique; v1 fed raw strand text only (strand time only on the wire as a pointer) — confirmed missed.
Nick chose option 2: the version is decided at the ceremony, never switched inside a live chain; existing friendships migrate by one re-key.
Built: crypto::chain::{BRAID_V1, BRAID_V2, strand_value, strand_bytes}; FriendshipChains.braid per era + RetiredEra.braid (stragglers open with their own) + braid_for_label; persisted "braid"/"retired_braid" (absent = v1); replication_subset carries it.
Agreement: offer field "braid" + clutch_offer_provenance_full folds a v2 claim beside the prior (unclaimed = byte-identical legacy provenance, so older peers still clutch on v1); clutch::agreed_braid = v2 iff both offers claim v2.
Claim gate: Contact::braid_claimable = EVERY folded device of the peer showed v2 in its sealed pong tail ("brd", DeviceEndpoint.braid, runtime only) — an older owner device would otherwise make ceremony ids never match.
Migration: pong edge that completes a friend fleet's v2 → arm_heavy_weave_for when current chains are v1 with an era (pickup respects ownership). Logs: "BRAID: ceremony for X weaves braid vN".
**Why:** under v2 a row's 32-byte S is all the braid needs, so text can be DISCARDED (never "shredded" — [[feedback_discard_not_shred]]) — the braid-safe redaction ticket that blocks real delete/truncate/nuke ([[project_contact_nuke_gaps]]).
**How to apply:** discarding a row's text must store S (strand_value of the row's time + ident_bytes) before dropping text, and only on v2 eras; v1 eras still need text for rows inside the last-256 incoming window. Docs: docs/braid.md §6.2a.

**v107 OUTAGE 2026-09-28 (Nick↔Emma, no messages or waves either way) — CONVICTED + FIXED:**
- The braid_ready heavy-weave migration re-armed on every restart: the friend claims v2 only when ALL of OUR devices show v2, so one older or refused device of ours kept the weave from ever landing.
- 3 ceremony_id mismatches tripped the breaker → repose_clutch_round NULLED friendship_id → Emma's side held no chains ("Loaded 0 friendships", "CHAT: cannot send — no friendship chain").
- Nick's phone kept offering a weave from era#0; Emma refused it ("we hold no era … offer not answered") → deadlock.
- Fixes (status.rs, conversation.rs):
  1. The migration is REMOVED; v2 arrives with any natural ceremony.
  2. The breaker keeps existing chains and discards only the round.
  3. A weave offer for an era we don't hold (Err("no era")) is answered as a FRESH ceremony (era_prior_claim = None).
- NEXT: a migration trigger must see BOTH fleets are v2 before arming.
