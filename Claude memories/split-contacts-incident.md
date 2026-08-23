---
name: split-contacts-incident
description: "CLOSED 2026-08-11 — census named it SHADOW CONVERSATIONS: receive arms derived conversations from chains.participants() while loader/persist/serve derive thru the contact; fixed at all three arms (621f335)"
metadata: 
  node_type: memory
  type: project
  originSessionId: 296bef77-7c97-45b8-8d90-dc492f93e557
  modified: 2026-08-23T17:06:58.270Z
---

**RESOLUTION (2026-08-11, photon 621f335 + 75b8fad):** the census proved the real bug — three receive arms (braid commit, ChatMessage, MessageAck) materialized conversations from `chains.participants()` while every other path (loader, persist snapshot, page server, census, walk digest) derives thru the CONTACT. A stale-era ceremony's participant set minted a SHADOW object: live rows rendered all session, died at restart (the 92-RAM/7-disk device). All three arms now resolve thru the contact + loud `SHADOW SEAM` log. Same-day round 2 PROVED the fix (97 rows loaded at boot where 8 had) and closed three more: (a) the braid commit's party-id seam broke SILENTLY on a stale participant set (neither our pid listed) — the behind device decrypted the same frame every ~15s forever, no ACK/row/persist; the seam now resolves the peer by contact match, loudly; (b) digest kick + friend history route no longer require `chain_woven` (capability = history key; stale woven flags stranded an 8-row device against an advertised 109); (c) **the "three self-contact stub rows" claim was WRONG — they were ordinary FLEET SIBLINGS** (the fleet shares one handle_proof by design); the census detector now excludes + marks siblings; the boot purge and roster-merge self-guard remain as backstops but found nothing real. LAN-record hole also closed: the record's LAN slot prefers the source of our own looped-back beacon (`OurLanAddrObserved`) over `get_local_ip`'s internet-interface guess. **History loss is real and partial: rows never durably written anywhere (shadow-era RAM) are unrecoverable.** Watch: census counts grow; SHADOW SEAM lines = chains worth re-keying. Original notes below.

Field state (2026-08-10 logs, Nick desktop 1be949c1 + laptop fe46a74b + Emma 8b29d719): a device advertises 92 rows (RAM digest) while serving 3 (disk table); the desktop's full resync walk toward Emma dropped EVERY page ("rid unmatched and sender is not a fold-trusted sibling"); the desktop runs re-CLUTCH ceremonies toward SIX Pending contacts (573KB offers at offline devices). Diagnosis: contact rows duplicated/reset — each affected pair's conversation is SPLIT across the old and new contact key; the visible conversation holds only recent rows, the history sits under the other key's table (or only in one device's RAM). Fleet sweeps agree at "0 new" because siblings mirror the same 3-row table.

Drop mechanism (fixed, f67cf8b): two contact rows resolving one peer → the page's token resolves to conversation A while the rid was registered on conversation B's in_flight → every page dropped. Fix: `hist_rid_map` (rid → conversation, consumed on match) — any page answering a request we minted merges.

Ground truth incoming: boot STORAGE census (f67cf8b) logs per-contact table id + loaded row count + a loud DUPLICATE CONTACT line (same handle_proof, different handle_hash). Read the NEXT Nick/Emma logs before designing the repair.

Repair design (pending census): contact dedupe (one row per identity — decide which key survives), orphaned-table re-home (merge old-key rows into the surviving conversation id — the migrate_conversation_tables shape), and root-cause WHY stubs merged. Prime suspect: the B4 roster-pull backstop (5f5fe13) made a previously-dead pull path live, landing a stale hub roster slot whose entries didn't match local contact keys; `reclutch_chainless_contacts` then reset ceremonies for rows whose chains no longer loaded. Related: [[messaging-solidity-phase-a]], [[lane-rotation-wedge-heal]], [[persist-findings-early]].

Do NOT: complete/encourage the stub ceremonies, remove-and-re-add contacts, or attempt table surgery before the census lands — a wrong merge direction permanently re-keys friendships toward stub identities.
