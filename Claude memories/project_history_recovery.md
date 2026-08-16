---
name: project_history_recovery
description: "Message-history sync: friend backfill (phase 1) SHIPPED @ 4ca82ed + E2E verified; fleet sync between own devices (phase 2) SHIPPED @ c6d6285 (2026-07-16), E2E pending"
metadata: 
  node_type: memory
  type: project
  originSessionId: cf9fbed2-aa87-4ebe-b7cf-fca16d308f75
---

Phase 1 (friend-sourced) landed 2026-07-09 @ commit 4ca82ed. After a client reset, once the fresh CLUTCH weave seals, the reset device pulls conversation history FROM THE FRIEND — newest-first, paged: head page immediate, background trickle to completion, scrollback jumps the queue. Storage-layer sync, NOT a messaging replay — the live braid never sees it (that's why the retransmit path, which only replays unACKed pending ciphertext, couldn't help). Fixes the "reset loses all history" gap and complements [[project_clutch_ui_thread_hitch]] (the weave-reset that unblocked re-key was the prerequisite).

Design that shipped:
- History KEY: crypto::clutch::derive_history_key — spaghettify(DOMAIN ‖ friendship_id ‖ pristine active chains [256..512], sorted) derived ONCE in FriendshipChains::from_clutch (both sides byte-identical only at ceremony birth). Persists as chains schema v6 (optional field, pre-v6 → None → recovery unavailable until re-key). Zeroized on re-key supersede (photon_app ~6498) + delete (~8824). Seals pages OUTSIDE the ratchet.
- Codec: src/network/history_pages.rs — KEY-AGNOSTIC (bare 32B key), kete ChaCha20-Poly1305, metadata (oldest_osc, more) inside the seal. Phase 2 reuses verbatim under the fleet key.
- Wire: standalone signed hist_req / hist_page in fgtw/protocol.rs (canonical sign_file + read_verified + parse_section_after_header — zero new raw parse sites, vsf-gate stays baseline 8). Ride PT like chat; packet-acked in BOTH status.rs RX branches (un-acked reliable type HOL-blocks chat).
- Serving: photon_app check_status_updates HistoryRequestReceived arm — token→key→other-participant→contact; require knows_device + is_mutual; rid dedup + cadence + stale-reject; load_message_page_before serves newest-first rārangi rows.
- Requester: kickoff on chain_woven edge in seal_chain_if_ready; drive_history_recovery ticks beside retransmit_due_messages; scrollback (MouseWheel dy>0) sets urgent; merge flips direction (recovered-outgoing ⇒ delivered=true), recovered=true, dedup by (timestamp, content), early-stop on intact re-key. Cursor persists as hist_oldest/hist_complete in CONTACT STATE (survives the chains file being replaced on re-key — only the key lives with chains).
- Storage: ChatMessage.recovered flag; load_messages now numeric-sorts keys (recovery inserts older rows later); save_messages_page + load_message_page_before.

Status: **E2E VERIFIED LIVE 2026-07-09** (Nick reset → re-CLUTCH → backfill, Nick desktop ↔ James Android — James is the USB-plugged phone; the test phone is a different Samsung): seal→full-history-restored in 145ms, 13/13 rows one page one round-trip; both sides kicked off (mutual re-key case), reset side served 0-rows/more=false and the intact side early-stopped correctly; rid dedup ate the alt-path duplicate (exactly one serve); ZERO errors/rejects/HOL-blocking either side. Same run also proved the weave-reset fix live: the fresh completion refired the probe that the stale probe_sent used to suppress (the old deadlock line "Ignoring duplicate proof — chain already woven" never appeared). Also green: 112 unit tests (key both-sides determinism, codec round-trip+tamper, pagination walk+load-sort, chains v6, provenance). No UI cue for recovered rows yet (deliberate).

Phase 2 (fleet sync between OWN devices) SHIPPED 2026-07-16 @ c6d6285, same codec/frames, routed by SENDER:
- Serve/receive arms: a hist_req/hist_page from a fold-trusted SIBLING device uses the FLEET key; friend traffic unchanged. Sibling pages merge VERBATIM (same identity — NO direction flip; friend pages keep the flip). Tokens resolve by DERIVATION from party ids (contact_idx_for_conversation_token; self notes = [our_pid, our_pid]) so no chain is needed — roster-merged contacts backfill pre-CLUTCH.
- Driver routes per request: friend route (online + woven + keyed chain) else any online sibling; ONE conversation-level cursor (hist_oldest/hist_complete) spans both sources.
- kick_fleet_history_sweep arms every friend/self conversation on sibling-online edge + after roster merge; early-stop keeps a no-change sweep at one page each.
- LIVE push: send/receive/self-note/ACK-delivered pushes the row(s) to online siblings as an unsolicited fleet-key page (unmatched rid from a sibling = push signature, cursor untouched); fresh merges gossip one hop onward, never back at the sender; `delivered` is monotonic across merges (tick shows fleet-wide).
- Same commit: roster CRDT (push_roster pull-MERGE-push, Contact.roster_updated LWW clock, tombstones honoured, reconcile push-back after pull) — contacts converge across the fleet too.
- Also fixed there: both hist arms resolved "other participant" against the raw identity seed while chain participants are PARTY IDS (pin-set migration) — friend recovery was sort-order dependent. E2E of phase 2 on real devices still pending.
- KNOWN SEAM LEFT: drive_blind_ops still derives friend conversation tokens from the raw seed (`our_seed`), not the party id — likely breaks blind frames the same way; untouched, flagged 2026-07-16.
