---
name: project-braid-v2
description: "2026-09-27 BUILT: braid v2 weaves S = spaghettify(domain ‖ strand_time ‖ len ‖ text) per strand, agreed per era at CLUTCH; no forced migration (v2 rides natural ceremonies); unlocks real delete; two v107 outages convicted + fixed"
metadata:
  type: project
---

Nick 2026-09-27: the weave must include a spaghettification of each woven message PLUS its time so every strand is unique (v1 fed raw strand text only).
Version is decided at the ceremony, never switched inside a live chain (docs/braid.md §6.2a).
Code: crypto::chain::{BRAID_V1, BRAID_V2, strand_value, strand_bytes}; `FriendshipChains.braid` per era + `RetiredEra.braid`; offer field "braid"; `clutch::agreed_braid` = v2 iff both offers claim it; `Contact::braid_claimable` = EVERY folded device of the peer showed v2 in its sealed pong tail ("brd").

**Why:** under v2 a row's 32-byte S is all the braid needs, so text can be DISCARDED ([[feedback_discard_not_shred]]) — the prerequisite for real delete/truncate ([[project_contact_nuke_gaps]]).
**How to apply:** discarding a row's text must first store S (strand_value of the row's time + ident bytes), and only on v2 eras; v1 eras still need text for rows inside the last-256 incoming window.

**v107 outages 2026-09-28 (Nick↔Emma), convicted + fixed — the rules they left:**
1. The heavy-weave migration re-armed every restart (one older device of ours blocked the claim forever) → REMOVED; v2 arrives with any natural ceremony. A future trigger must see BOTH fleets on v2.
2. The ceremony_id-mismatch breaker nulled friendship_id → it now keeps chains and discards only the round.
3. A weave offer for an era we don't hold (or a retired one) from a FRIEND is always answered as a fresh ceremony; only siblings wait/pull.
4. A straggling "First ACK" zeroized the NEW round's keypairs → zeroize only when clutch_state == Complete.
5. `Contact::note_offer_provenance`: one provenance per side; a new offer replaces that side's old one.
6. `RetiredEra.heavy` (persisted "retired_heavy"): Behind on a heavy-retired era + friend + owner → ConsentFresh.
Still open: a lagging sibling's stale pong era advertisement can make a friend arm a needless ceremony (self-heals; churn).
