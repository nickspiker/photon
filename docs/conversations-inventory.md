# Conversations convergence — the inventory (2026-10-24)

Every consumer of a friendship's pair chains/identity, classified BOND / CONVERSATION / both, mapped at commit 8e775d76 on branch `conversations` (line numbers drift as the work lands). The work map for docs/conversations.md §7; risk 1 (the token codec) is fixed by the stored token.


Labels: **B** = BOND, **C** = CONVERSATION, **B/C** = BOTH or unclear (reason given). **[mol: …]** names an existing molecule-path equivalent that could serve every conversation.

**One thing to fix before anything else:** `bond_split_of` sets `conversation_token = bond_id.token()`, but nothing persists that token for a non-molecule blob. `chains_from_vsf_bytes` (`src/storage/friendship.rs:207` → `from_storage_v5`, `src/types/friendship.rs:759`) recomputes it as `derive_conversation_token(participants)`. Only the molecule flag re-derives it from the id (`src/storage/friendship.rs:341-343`). So a bond blob reloaded from the vault, or received by chain_sync, comes back with the pair's conversation token. Chain_sync will then drop it at "inner token mismatch" (`src/ui/photon_app/status.rs:3969`). Persisting the token (codec v11) or adding a bond flag has to land with the split.

---

## `/mnt/Harbor/Code/photon/src/types/contact.rs`
- 577 `friendship_id: Option<FriendshipId>` — the one link from a person to "the" chains. **B/C**: today it is both the conversation id and the chain id. Split it into `bond_id` plus a list of conversations.
- 578-652: the `clutch_*` fields, `ceremony_id`, `ceremony_owner`, `offer_provenances`, `consent_mutual`, `knocked_session`, `completed_their_hqc_prefix`, `trust_level` — **B**.
- 620-626: `era_ephemeral`, `era_resp_cache`, `era_prior_claim`, `era_weave_due`, plus `peer_era_capable` (645) — **B** (the light ratchet and heavy weave move to the bond).
- 654 `owner_woven`, 671 `chain_woven`, 673-681 `probe_sent`/`probe_resends_left`/`their_probe_seen`/`their_probe_ceremony`/`chain_advanced_by_ack` — **B**. They prove the CLUTCH channel.
- 699 `chain_fail_streak` — **B/C**. It counts garbage decrypts on whichever chain failed, so it needs one counter per chain.
- 701 `ceremony_mismatch_streak`, 705-707 `last_chain_reset_*` — **B**.
- 703 `gap_streak` — **C** (conversation lane transport). 709 `digest_kick_osc` — **C** (anti-entropy cooldown, should move onto the Conversation).
- 569-571 `wfd_cred*`, 717-723 `blind_*` — **B**.
- 725-727 `molecule`, `molecule_remote` — the group-view hack.
- 890-900 `molecule_view` sets `friendship_id = gid`, so every `friendship_id.is_some()` gate passes for a group view. **C** (delete it at Phase 2).
- 915-920 `Contact::conversation(&us)` — the id is `[us, them]` derived, or the molecule id for a view. **C** [mol: `Conversation::new_molecule`].
- 926 `is_reachable`, 931 `is_keyed`, 937 `remote_count` — **C**. `is_keyed` reads `clutch_state`, which is **B** state; after the split it should mean "the conversation holds keys".
- 1155 `void_weave_seal_from_previous_chain` — **B**.

## `/mnt/Harbor/Code/photon/src/types/friendship.rs`
- 91-104 `BondId::derive`/`token` (new) — **B**.
- 118 `FriendshipId::derive` — **C**. It stays the pair conversation's id and table.
- 481-577 `from_clutch_woven`: the id at 492 and token at 495 come from the pair. The history key and lane root (513/515) are bound to the **pair FriendshipId**, and the weave (531) runs `derive_era_keys(friendship_id, …)`. Today **B/C**. After the split CLUTCH must produce bond chains, so this should take a `BondId`.
- 580 `from_molecule_root` — **C** (the general conversation-keys constructor).
- 620 `bond_split_of` — **B** (the token caveat above applies).
- 631/658 `from_storage_v3` and 743/759 `from_storage_v5` recompute the token from participants — **risk**, see above.
- 947 `other_participant` — **B/C**. It is the 2-party sender attribution (refuses anything not exactly 2). [mol: fold-over-standing attribution]
- 1162 `supersede_with`, 1375 `era_superseded_by`, 1357 `sanitize_replicated`, 1476 `replication_subset` — generic chain machinery; serve both.
- 1626 `clear_pending_up_to`, 1637 `rearm_pending_after`, `our_lane_wedged_at_peer_anchor` — **C**, but they don't know about pending `targets` (see Risks).
- 1894 `set_pending_targets`, 1907 `retarget_pendings`, 1931 `process_group_ack` — **C** [mol: the per-member ledger that should serve all sizes].
- 1950 `process_ack` (hash-matched, single peer) — **C** (pair form).
- 2123 golden test pins `derive_conversation_token`.

## `/mnt/Harbor/Code/photon/src/types/conversation.rs`
- 57 `new` (participant-derived id) and 77 `new_molecule` — **C**.
- 95 `is_molecule`, 100 `set_participants` (molecule-only) — **C**.
- 115 `anti_entropy_digest`, 186 `remote_count` — **C**.

## `/mnt/Harbor/Code/photon/src/crypto/clutch.rs` and `/mnt/Harbor/Code/photon/src/crypto/era.rs`
- clutch 18 `derive_conversation_token` — **B/C**. Today it is the pair's single wire token. It stays the pair conversation's token, stored as data; the bond gets `BondId::token`.
- clutch 71 `derive_history_key`, 89 `derive_lane_root` — both take `friendship_id`, so whatever id the CLUTCH output is born under is bound into the keys. **B** after the split.
- clutch 145 `identity_friendship_secret` (pong seal and ceremony eggs) — **B** (identity-level, no chains).
- era 263 `derive_era_transcript(token, …)` hashes the wire token — **B** for the light ratchet.
- era 282 `derive_bond_split`, 288 `derive_era_keys(friendship_id, …)` — both.

## `/mnt/Harbor/Code/photon/src/ui/photon_app/conversation.rs`
- 226 `our_party_id`, 246 `has_remote`, 252 `is_zero_remote` — **C**.
- 263 `conv_of(ci)`, 543 `conv_mut_of(ci)` (materialize, then `load_messages`) — **C**. Must become keyed by conversation id. [mol: id lookup + `new_molecule`]
- 281 `active_contact` (contact found from the active conversation id) — **C** [mol: 291 `active_molecule`].
- 329 `open_conversation_with(ci)` — **C** [mol: 300 `open_molecule_conversation`].
- 351-402 `broadcast_focus_claim`: the token is re-derived as `derive_conversation_token([us, c.handle_hash])` at 366 — **C**. There is no molecule claim (comment at 314).
- 500 `lane_transmit_capable(ci)` (via `friendship_id` → `lane_capable`) — **C**.
- 515 `compose_ready`: pair branch reads `chain_woven`, `lane_transmit_capable`, `sibling_sendable` — **B/C**. It gates conversation compose on bond weave state. [mol: 517-522, phase + `lane_capable`]
- 564 `update_sync_records`: one record per `friendship_chains` entry (molecules, siblings and later bonds included). The digest comes from the conversation with id == fid — **C**. A bond has no rows, so it would publish digest 0.
- 604 `reclutch_chainless_contacts` (missing chains → `repose_clutch_round`) — **B**. A lost *conversation* key should become a stranded-member re-delivery, not a re-CLUTCH.
- 632 `repose_clutch_round` (nulls `friendship_id` at 663) — **B**. Nulling the shared id also strands the conversation.
- 670 `break_wedged_ceremonies` (`has_chains` via `friendship_id`) — **B**.
- 695-745 `spawn_next_pending_keygen`, heavy-weave pickup: claim `era_tag` from `friendship_id` chains (728-731) — **B**.
- 1084 `clear_molecule_unread` / 1097 `clear_unread(ci)` — **C** [mol: 1084].

**`commit_braid_rx`, 1211-2207:**
- 1266 finds chains by token — both.
- 1280-1327: attribution via `other_participant`, with the "SHADOW SEAM" contact fallback — **B/C** [mol: 1280-1291 standing fold].
- 1345-1381: `conv_pos` resolved through the contact's derived id — **C** [mol: 1345-1354 by id].
- 1398-1450: `latest_lane_by_signer` keyed `(friendship_id, signer)` — **C**.
- 1403-1440 garbage decrypt — **B/C**:
  - sibling → `initiate_sibling_chain_reset` (**B**);
  - friend → `fork_friend_rekey`, i.e. re-CLUTCH (**B** action used to repair a **C** fork);
  - [mol: 1411 "held for the era owner's refresh"].
- 1540-1553: a strand miss arms the friend history walk — **C**.
- 1566 `rx_lane_key_pre_advance` (the wave basket egg) — **C/waves**.
- 1590: the 256-row cadence fires `cadence_ci` — **B/C**. It counts conversation rows but triggers the pair light ratchet [mol: 2113-2121 `mint_molecule_era`].
- 1668-1673 Era row: inserted into the pair conversation table, then `on_era_signal` — **B** (the row belongs on the bond).
- 1674-1681 Molecule control row (Offer/Join/Wrap/Decline into the friendship conversation; Records/Wrap into the group's) — **B** for invites and joins, **C** for Records.
- 1682-1689 Wave row — **C**.
- 1693-1735 Delete marker (tombstone, `offer_undone` for bond offer rows) — **C** (offer rows are **B/C**).
- 1739-1750 Probe: sets `their_probe_seen` and stores a hidden Probe row in the conversation — **B** (row should live in bond state).
- 1751-1760 a real message seals woven / `chain_advanced_by_ack` — **B** side effect of **C** traffic.
- 1762-2011 bridge capture (siblings) — **B** (sibling bond).
- 1830-1906: `looking`, `claimed_elsewhere` (fleet claim compares the frame token), `behind_held`, `will_ding`, focus-claim drop — **C**.
- 2016-2026: `attach_expect_pushes` (ContactId) and the `unread_count` bump — **C**.
- 2029-2065: notify and chirp seeded by `relationship_digest(from, us)` — person-level; keep.
- 2082-2092: durable ACK snapshot (`AckRequest` with the frame token) — **C**.
- 2104-2108 → `on_era_signal` (**B**) and `on_molecule_signal` (**B**/**C**).
- 2122 `repair_dispatch(ci, CadenceReached)` — **B** [mol: 2113-2120].
- 2133-2137: persist `persist_molecule_conv` vs `persist_messages_async(ci)` — **C** [mol].
- 2149-2157: sibling push by token vs `push_rows_to_siblings(idx)` — **C** [mol].
- 2159-2163: `seal_chain_if_ready` (**B**), `resend_held_messages` (**C**).
- 2164-2169: sibling reset (**B**) and friend re-key (**B**).

**`drain_chain_syncs`, 2210-2396:**
- Era supersede and lane merge — generic, both.
- 2249: the booted-orphan check uses `contacts.friendship_id`.
- 2295 molecule roster merge [mol].
- 2304-2329: wires `contact.friendship_id` from the replicated participants — **B/C**.
- 2349-2390: `contact_idx_for_conversation_token` flips the contact Complete and woven — **B** (stays bond-only once the token is the bond's).

**`drain_history_pages`, 2399-2854:**
- 2420-2440 molecule sibling-page branch [mol: no friend route for groups].
- 2441-2450: token resolves to a contact via `other_participant` or the pair-derived token — **C**.
- 2459-2482: decrypt-fail park on `history_recovery` — **C**.
- 2509-2729: merge rows — **C**.
- 2731-2778: summary chirp — **C**.
- 2780-2817: wave stop edges from siblings, recording fetch — **C**.
- 2821-2830: `persist_messages_async(idx)`, `push_rows_to_siblings(idx)` — **C**.
- 2832-2851: fleet-forward drain gated on `chain_woven || lane_transmit_capable`, pending read through `friendship_id` — **C** (the `chain_woven` gate is **B**).
- 2857 `summary_chirp_and_flag(idx, conv_pos)` — **C**.
- 2903 `sweep_undischarged(token)` (token resolved to a contact via the derived pair token, 2909) — **C**.

## `/mnt/Harbor/Code/photon/src/ui/photon_app/messaging.rs`
- 74-100 `submit_message`: molecule branch [mol] or the contact path at 106. 115-140 bridge prompt gate (**B**). 150-153 empty send = liveness ping (**B**/presence). 158 react, 175-193 edit/reply/BridgeCmd — **C**.
- 203 `send_chain_message(ci, …)`:
  - 217-224 `remote_count` → zero-remote path (246 `conv_mut_of`, 253 signalled persist);
  - 278-288 bubble insert, `persist_messages_async(ci)`, `pending_chain_sends` (ContactId).
  - **C** [mol: `send_molecule_message`, `molecules.rs:905`].
- 293 `send_control(ci, ctl, ts)` → `chain_transmit_with` — **B/C**, decided by control kind: Probe and Era are **B**; Delete and Wave are **C**.
- 298 `persist_messages_async(ci)`, 331 `persist_messages_signalled(ci)` (sibling skip at 333, late hydration at 341-363) — **C** [mol: 304 `persist_conversation_async`].
- 511 `drain_persist_done`: `conv_id` → ci (540-544) → `push_rows_to_siblings(ci)` — **C**.
- 555 `persist_chains_async(fid)`, 564 `persist_chains_then` — generic, both.
- 671 `drain_braid_tx`:
  - `done.friendship_id` → contact at 677 — **C**;
  - 709 sets molecule targets [mol];
  - 712-742 wave offer lane-key capture plus `send_express_signal(ci)` — **C/waves**;
  - 754-776 one `MessageRequest` per route with `conversation_token` — **C**.
- 797 `drain_pending_chain_sends` (ContactId → ci) — **C**.
- 844 `bridge_wire_for_row(ci)` — **B** (sibling bond).
- 869 `resend_held_messages(ci)` (excludes Era and Molecule control rows at 882) — **C** [mol: `resend_held_molecule_rows`, `molecules.rs:1096`].
- 906 `chain_transmit(ci)` and 918 `chain_transmit_with(ci, …)` — **B/C** (callers carry both bond and conversation traffic) [mol: 1008 `molecule_transmit`].
- 942 `route_for_contact(ci)`: needs `friendship_id`; Complete or lane-capable gate; one Route with `party = handle_hash`; `conv_id = contact.conversation().id()` — **B/C** [mol: 977 `routes_for_molecule`].
- 1039 `transmit_core(friendship_id, conv_id, routes, …)` — shared core, keyed on both ids.
  - 1083-1113 in-flight window and idempotency read the chains by fid;
  - 1118-1155 weave strands come from `conv` rows (anchor-only for siblings and molecules);
  - 1159/1220 `send_encrypt_busy` keyed fid.
  - Already the shared core; a conversation-only version would drop `friendship_id`.
- 1260 `maybe_send_chain_probe(ci)` — **B**.
- 1294 `seal_chain_if_ready(ci)` — **B**. It also arms friend history recovery (1341-1361, **C**) and pushes the roster (1371-1376, **B**).

## `/mnt/Harbor/Code/photon/src/ui/photon_app/era.rs`
Everything here is **B** today: it ratchets the shared chains. After the split it moves to the bond, and conversations use the mint instead [mol: `mint_molecule_era`, `molecules.rs:279`].
- 64 `friendship_repair` (pure table), 114 `classify_peer_era`, 152 `era_owner`.
- 167 `recompute_ceremony_owners` (per non-sibling contact).
- 227 `repair_dispatch(ci, trigger)`: fid from contact at 229; token and `era_peer_seen` (239-254); `build_chain_pull_vsf(token)` era_pull at 277.
- 304 `propose_light_ratchet(ci)`: fid at 323; `initiator = participants[0]` at 332; `chain_transmit_with(ci, Era Nudge|Init)` at 336/347.
- 358 `on_era_signal(ci, …)`: fid at 361; stranded check → `rekey_without_chains(ci)` at 390; `derive_era_transcript(chains.conversation_token)` at 414/465; `derive_era_keys(fid)` at 417/468; Resp at 429; cutover at 473; Nudge at 484-501.
- 507 `arm_heavy_weaves` (`friendship_id.is_some()` at 511), 524 `arm_heavy_weave_for(ci)`.
- 553 `era_pull_miss_from(token)` (token → contact via `friendship_id` at 569-572).
- 579 `era_cutover_flush(ci, fid)` — **B/C** split already present [mol: 582-584 `after_molecule_cutover`].

## `/mnt/Harbor/Code/photon/src/ui/photon_app/ceremony.rs`
All CLUTCH, so **B**:
- 6 `spawn_clutch_keygen`, 54/111 encap/decap spawns.
- 158 `check_clutch_kem_decaps` (pair token at 259).
- 403 `check_clutch_keygens` (zero-remote guard at 472; offer token 607-611 → `build_clutch_offer_vsf`; KEM token at 707).
- 816 `check_clutch_kem_encaps`.
- 310 `spawn_clutch_ceremony`: `from_clutch_woven([us, them], eggs, prior, braid)` at ~356 produces chains under the **pair FriendshipId** with the pair token. **This has to emit bond chains (BondId) after the split.**
- 963 `check_clutch_ceremonies`:
  - stale-woven check against the fid's era at 1013-1024;
  - `friendship_id = result.friendship_chains.id()` at 1027;
  - `fanout_pairs::store` at 1041 (device pair secret);
  - supersede or insert into `friendship_chains` at 1067-1091 (today this supersedes the *conversation* era);
  - `contact.friendship_id = Some(fid)` at 1101;
  - proof `ClutchCompleteRequest` with `result.conversation_token` at 1133;
  - woven/probe reset at 1177-1180;
  - persist + proof at 1230; probe at 1234.
- 1245 `complete_clutch_ceremony_by_idx`: pair token at 1372; `claim_fid = contact.friendship_id` at 1425; `EraPrior` from the claim fid's chains at 1449-1460 (the heavy weave must read the **bond** era).

## `/mnt/Harbor/Code/photon/src/ui/photon_app/sync.rs`
- 8 `boot_contact`: removes the contact and deletes the `friendship_id` chains (64-70) — **B/C**. Today the pair conversation's keys die with the friendship; the doc's Phase 3 wants the conversation to outlive the bond.
- 123/144/179 self-row helpers (zero-remote) — N=1 special cases.
- 617 `retransmit_pending_clutch_proofs` (pair or sibling token at 662) — **B**.
- 690 `send_friend_knock` (pair token at 705-706 → `build_friend_knock_vsf`) — **B**.
- 753 `resend_clutch_offer` (pair token at 804) — **B**.
- 857 `retransmit_due_messages` — **C** [mol: the molecule pass at 899-948 is the per-member, per-route general form]:
  - route snapshot per contact `friendship_id` (867-898);
  - pair pass at 953-997;
  - bridge lane rotation for sibling fids at 999-1026 (**B**);
  - give-up stamping via sibling `friendship_id` at 1027-1070 (**B**);
  - backoff collapse at 1073-1082;
  - doorbell via `friendship_id` at 1087-1109 (presence, **B**).
- 1113 `contact_idx_for_conversation_token` (derives the pair token for every non-sibling contact) — **B/C**. It is used for both bond and conversation token resolution.
- 1124 `rekey_without_chains(ci)` (`friendship_id = None`, new keygen) — **B**.
- 1149 `drive_chain_replication`:
  - sibling fids excluded at 1168-1173;
  - per fid `chain_pushed_osc`/`lane_pushed_pos`;
  - molecule roster-only frames at 1204-1210 [mol];
  - `build_chain_sync_vsf(subset.conversation_token)` at 1298.
  - **B/C** (generic; bonds and conversation keys both replicate; depends on the token persistence fix).
- 1326 `push_rows_to_siblings(ci, …)` (pair token derived at 1340) — **C** [mol: 1346 `push_rows_to_siblings_token`].
- 1474 `apply_sibling_chain_reset` (`from_clutch([sib pids], fleet eggs)`, sets `friendship_id`, `build_chain_reset_vsf(token)`) — **B**.
- 1576 `initiate_friend_rekey`, 1611 `initiate_sibling_chain_reset` — **B**.
- 1626 `reserve_fleet_forwards` (contacts loop, `chain_woven` gate, `push_rows_to_siblings(ci)`) — **C**.
- 1667 `kick_fleet_history_sweep` (contacts loop, `conv_mut_of`) — **C**. It misses molecules.
- 1711 `drive_history_recovery` — **C**. It misses molecules.
  - candidates via `dm_conversation` at 1803;
  - friend route: `friendship_id` → `history_key` + `chains.conversation_token` at 1811-1829;
  - fleet route: derived pair token at 1840;
  - park fingerprint from the `friendship_id` history key at 1871-1875;
  - `hist_rid_map` keyed `ConversationId` at 1934.
- 1959 `drive_blind_ops` (`chain_woven` gate at 1983; pair or sibling token at 2009-2015) — **B**.
- 2065 `maybe_generate_s` (`chain_woven`) — **B**.
- 2099 `ping_contact` — presence, **B** (no chains).

## `/mnt/Harbor/Code/photon/src/ui/photon_app/status.rs` (app side of the RX drain)
**Online/pong arm, 471-1180:**
- 487-717: each `sync_records` entry matched by token over `friendship_chains`, then:
  - tip computation, lane wedge rotation (509), clear/rearm pendings (533/537), stuck-tip rotation (551) — **C**, no member awareness;
  - era observation → `era_triggers_after` (517-524, **B**); retire edge (526);
  - fid → contact via `friendship_id` (563-567, non-sibling) for the digest, `peer_ahead`, replicated tick, history kick and re-serve (570-713) — **C**. Molecules are skipped here because no contact holds their fid.
- 1019-1135: deadlock KEM re-request, consent knock, full offer send with pair token at 1060 — **B**.
- 1137-1163: `came_online` retransmit request via `friendship_id` — **C**.

**ChatMessage arm, 1182-1537:**
- chains found by token (1209);
- party-id seam (1217); sender via `other_participant` (1242) [mol: 1228-1240];
- era routing and pending cutover → `rotated_flush` (1253-1273);
- contact by handle_hash (1280); `peer_era_capable` latch (1301, **B**); device trust (1308);
- `conv_pos` via the contact-derived id (1316-1353) [mol];
- dup/re-ACK (1358-1422) and gap buffer (1426-1470), with the friend history walk excluded for molecules (1442);
- off-thread decrypt (1481-1523) — all **C**;
- unknown token → `rekey_probe` (1529-1535), molecule-excluded — **B**.

**MessageAck arm, 1538-1785:**
- molecule early-out to `molecule_acks` (1545) [mol];
- `other_participant` (1590) → contact → `conv_pos` via the contact (1626-1654);
- `process_ack` (1657) and Resp-ACK cutover (1663, **B**);
- `chain_advanced_by_ack` / proof resends (1671-1675, **B**);
- keypair zeroize (1679-1715, **B**);
- delivered flag (1739-1760) and `push_rows_to_siblings(contact_idx)` (1764) — **C**.
- **B/C**: today one ACK path serves both the probe/era rows and chat rows.

**CLUTCH arms — all B:**
- 1803 ClutchOfferReceived: pair or sibling token match at 1857; the era-prior claim reads `friendship_id` chains (1907-1943); consent flip (1940+); re-key keeping the chains (2061, 2428); offer and KEM tokens (2211).
- 2524 ClutchKemResponseReceived (2562).
- 2737 ClutchCompleteReceived (2774, woven reset at 2873, 2955).

**HistoryRequestReceived, 3243-3446 — C:**
- friend route: token → `history_key` + other participant → mutual contact (3280-3305);
- fleet route: `contact_idx_for_conversation_token` (3317);
- serves `load_message_page_before(&their_seed)` (3344), i.e. the pair-derived table.

**Attachment arms — C:**
- 3458/3521/3601/3713: `attach_wire_key(sender, token)` at 3469/3529/3610/3748; replies rebuilt with the request token (3782-3891).

**Other arms:**
- 3918 ChainSyncReceived — **B/C**. Sibling-gated, epoch key, inner token check at 3969 (the codec risk).
- 4066 FriendKnockReceived — **B**. Pair-derived match (4078) or `stranger_knocks`.
- 4139 FocusClaimReceived — **C** (token stored opaque).
- 4211/4224 ChainPull and ChainPullMiss — **B/C** (token).
- 4360 ChainResetReceived — **B** (expected sibling token at 4389-4395).
- 4408 HistoryPageReceived — **C**. Friend key = chains `history_key` found by token, other participant → contact (4485-4505); sibling → `contact_idx_for_conversation_token` (4479).
- 4548 BlindFrameReceived — **B** (pair or sibling token at 4589; the sibling S seal uses the sibling chains' `history_key` at 4608 and 4885).

**Post-drain:**
- 5254 `persist_hashes` → `persist_messages_async` by handle_hash — **C**.
- 5263 `rotated_flush` → contact by `friendship_id` → `resend_held_messages` — **C** [mol: none, a molecule cutover here goes nowhere].
- 5275 `reserve_jobs` — **C**.
- 5306 `rekey_probe` → pair-derived contact → chain_pull or `rekey_without_chains` — **B**.
- 5357 chain_pull serves — **B/C**.
- 5405 `era_triggers_after` → contact by `friendship_id` → `repair_dispatch` — **B**.
- 5428-5470 chain_pull miss → re-key — **B**.
- 5608-5617 probe, seal, held resend, chains persist — **B/C**.
- 5619 `molecule_acks` [mol].
- 5634 sibling chain reset apply — **B**.
- 5655 `claim_sweeps` — **C**.
- 5663-5710 `retransmit_requests` (fid) — **C**.
- 5820-5834 re-probe sweep — **B**.

## `/mnt/Harbor/Code/photon/src/network/status.rs` (RX worker)
- 38 `friend_pong_seal_key(identity DH)` — **B** (person; no chains).
- 166/173 `set_molecule_devices` / `molecule_admits(token, dev)` — **C** [mol: token-scoped admission, the general door for never-friended members].
- 3264-3279 the pong tail carries **every** `SyncRecord` to every pinging device, unfiltered — **C** per record.
- 3622-3720 ChatMessage: contact-device or `molecule_admits` gate; provenance = `BLAKE3(token ‖ prev)` (5080) — **C**.
- 3722-3757 MessageAck: provenance binds the token (5089); forwards `sender_pubkey` — **C**.

## `/mnt/Harbor/Code/photon/src/network/fgtw/protocol.rs` (wire, see section 3)
196 `SyncRecord`; 407-458 chat and ACK `tok`; 1466/1754/2080 CLUTCH; 2251 knock; 2284/2388 chain_pull and miss; 2423/2516 history; 2594 chain_sync; 2723 focus; 2911-3256 attach_*; 3076/3123/3399 pigeon; 3499 chain_reset; 3695-3762 blind.

## `/mnt/Harbor/Code/photon/src/storage/friendship.rs`
- 23 `chains_key(friendship_id)`, 28 save, 174 load, 388 delete — generic. A bond is stored under BondId bytes, so no collision.
- 51 `chains_to_vsf_bytes`: writes `friendship_id` and participants but **no token** for non-molecule blobs; molecule extras at 109-160.
- 207 `chains_from_vsf_bytes`: token recomputed from participants; molecule flag → `MoleculeId::token` (341-343). **Risk.**
- 363 `load_all_friendships(&[fid])`: callers enumerate `contact.friendship_id`, so bonds need their own enumeration.

## `/mnt/Harbor/Code/photon/src/storage/contacts.rs`
- 162/241/473 contact state persists `friendship_id` — **B/C**.
- 165/273/492 persists `chain_woven` (load latches probe flags) — **B**.
- 177/368/577 `owner_woven` — **B**.
- 842-899 CLUTCH keypairs and slots keyed by handle_hash — **B**.
- 918 `conversation_table(participants)` = `Conversation::new(..).id()` — **C**.
- 1089 `save_messages(conv)` and 1182 `load_messages(conv)` keyed by `conv.id()` — **C** (already conversation-keyed; works for molecules).
- 1258 `conversation_state_record` = `vault_key("conv_state", conv.id())` — **C**.
- 1369 `load_conversation_state(conv, legacy_contact_key)` (legacy fallback by handle_hash) — **C**.
- 1436 `save_messages_page(their_identity_seed)` and 1478 `load_message_page_before(their_identity_seed)` — **C**, but the table is derived from `[our_pid, their_seed]`, pair-only. Must take a conversation id.

## Other UI files
- `/mnt/Harbor/Code/photon/src/ui/photon_app.rs`:
  - 629 `BraidTxEncrypted.friendship_id`, 644 `Route{party}`, 1699 `friendship_chains` (one registry for siblings, pairs and molecules).
  - Fields keyed by fid or token: 1674 `send_encrypt_busy`, 1689 `hist_rid_map`→ConvId, 1697 `history_serve`(token), 1709 `bond_offers`, 1857 `attach_expect_pushes`(ContactId), 1981 `fleet_focus_claim`(token), 2025 `stranger_knocks`(**B**), 2029 `latest_lane_by_signer`, 2043-2047 `lane_rearm_cycles`/`lane_reserved_rows`/`lane_reserve_bursts`(fid, **C**), 2126 `pending_chain_sends`(ContactId), 2281 `era_reclutched`(party, **B**), 2293 `selected_msg` and 2301 `pending_delete` (ContactId, **C**), 2328 `chain_pushed_osc`, 2332 `chain_pull_sent`, 2336 `peer_era_seen`(fid,dev), 2338 `era_pull_sent`, 2340 `era_peer_seen`, 2342 `lane_pushed_pos`.
  - 3788 wave callable (`chain_woven || friendship_id`).
  - 4181 `dm_conversation` — **C**.
  - 4211-4254 contact status line (owner_woven / chain_woven) — **B**.
- `/mnt/Harbor/Code/photon/src/ui/photon_app/wave_ui.rs` — waves:
  - 316 `start_wave(ci)`; 842 `on_wave_signal(ci, …)` (peer = contact handle_hash).
  - 1184 `send_wave_signal(ci)`: `send_control`, `conv_mut_of`, `persist_messages_async`, `push_rows_to_siblings`.
  - 1204 `send_express_signal(ci)` (`friendship_id` → `lane_root` + `history_key`, current plus retired era, 1214-1252).
  - 1373 `drain_express_signals` (trial-opens over **all** `friendship_chains`, maps fid → non-sibling contact at 1406).
  - 1488 `derive_secret_for` (requires `friendship_id` at 1497).
  - 1507 `end_wave` (peer → `contact_index_by_handle_hash` → conversation, 1573-1588).
  - 1627 `ring_alert(ci)`; 1743 `drain_wave_keep` (1762-1802 env row over `chain_transmit(ci)`, `send_attach_blob(ci)`); 2003 `spawn_wave_engine(ci)` (addresses from the contact, **B**); 2114 `contact_index_by_handle_hash`.
  - Classification: **B/C**. Rows are **C**, the keys come from the chains (section 2), addressing comes from the person.
- `/mnt/Harbor/Code/photon/src/ui/photon_app/attachments.rs` — **C** throughout, no molecule path:
  - 178 `attach_send_now(ci)` → `send_chain_message`;
  - 238 `attach_wire_key(device, token)` (sibling → fleet key, else chains `history_key` by token);
  - 254 `send_attach_blob(ci)` and 327 `send_attach_chunks(ci)` (token via `friendship_id`, 271-279/335-343);
  - 408 `attach_fetch(sci)` (token via `friendship_id`, else handle_hash, 419-427; targets are the friend's devices and siblings);
  - 611 `replicate_sweep` (634), 700 `wave_export_start(sci)`.
- `/mnt/Harbor/Code/photon/src/ui/photon_app/bridge.rs` — **B** (sibling bond, per D4):
  - 360 `open_bridge_conversation` (`conv_mut_of`, rotate the `friendship_id` lane at 375); 418/450/495/592/760/896/1137/1197 `send_chain_message(ci)`; 571 `bridge_inflight_target(conv_of)`; 597-616 `reset_bridge_shell`; 733 pending check.
  - 906-917 `dispatch_pigeon_chunks`: token = `c.handle_hash`, fleet key.
- `/mnt/Harbor/Code/photon/src/ui/photon_app/molecules.rs`:
  - 27 `on_molecule_signal(ci, cp, …)` (the `in_group` dispatch has to become "arrived on a bond");
  - 45 `park_bond_offer(ci, row_osc)`; 107 `bind_offer` (Join over `chain_transmit_with(ci)` at 149); 178 `answer_bind`;
  - 264 `send_wrap_over_friendship` (273); 611 `send_bond_offer` (offer row into the pair conversation at 644-647, wire at 648); 659 `decline_offer` (676-680); 686 `forget_offer`; 713 `undo_invite`.
  - Classification: **B** wire; the offer-card rows in the pair table are **B/C**.
  - 279 `mint_molecule_era`, 905 `send_molecule_message`, 933 `drain_molecule_posts`, 842 `on_molecule_ack`, 1068 `after_molecule_cutover`, 1096 `resend_held_molecule_rows` — **C** [these are the molecule-path equivalents].
- `/mnt/Harbor/Code/photon/src/ui/photon_app/driver.rs`:
  - 1419-1760 strip actions via `strip_target()` → contact ci: react 1466, fetch 1550, star 1571-1578, resend 1618-1660, delete arm 1712, export 1718, wave 1723, fetch-hint 1750-1754 — **C**, contact-only (no molecule path).
  - 3086-3160 deferred delete (`conv_mut_of(sci)`, `push_rows_to_siblings`, `send_control(Delete)`) — **C**.
  - 4554-4690 boot: conversations per contact, census, chains loaded by `contact.friendship_id` (4683-4689), then `load_all_molecules` — both.
- `/mnt/Harbor/Code/photon/src/ui/photon_app/render.rs` — **C** unless noted:
  - 157-180 `active_contact`/`molecule_view`/offer cards via `conv_of(ci)`;
  - 416-436 wave pill (`chain_woven || friendship_id`);
  - 2075 Ready sort via `dm_conversation`;
  - 2367 / 2407 panel remote count and bonds list (`friendship_id.is_some()`);
  - 2480 panel era via contact `friendship_id`;
  - 2563 history line; 2789 viewer rows;
  - 2962-2981 `is_self_contact` and conversation id;
  - 3088/3484 `chain_woven` gates — **B**;
  - 7376-7387 `RowView::of_contact` (unread via `dm_conversation`).
- `/mnt/Harbor/Code/photon/src/ui/photon_app/devices.rs`:
  - 250-262 ckpt conversation roots (contacts loop); 569 row cadence — **C**, both miss molecules;
  - 986 / 1365 sibling and tombstone chain deletes (`friendship_id`) — **B**;
  - 1306 / 1412 / 1469 roster `woven` — **B**;
  - 2325-2343 pong seal keys (sibling/zero-remote vs identity DH) — **B**.
- `/mnt/Harbor/Code/photon/src/ui/photon_app/manage.rs` 57 `clear_active_history` — **C**. Pair branch `persist_messages_async(ci)` + `push_rows_to_siblings(ci)` at 104-110 [mol: token branch at 111-117].
- `/mnt/Harbor/Code/photon/src/ui/photon_app/launch.rs`: 248-260 and 320-330 chain merge by `friendship_id`; 460-466 `is_self` forces Complete (N=1); 484-495 `stranger_knocks` pair token (**B**).
- `/mnt/Harbor/Code/photon/src/network/handle_query.rs`: 736-741 chains by `friendship_id`; 855-866 one conversation per contact plus the legacy conv_state key.
- `/mnt/Harbor/Code/photon/src/ui/photon_app/conv_panel.rs` 140 Boot → `boot_contact`.
- Contact-only enumerations that should become conversation-id based (all **C**): `drafts.rs:101` (sibling fid = sibling conversation id), `holdings.rs:332`, `settings.rs:677`, `viewer.rs:109,238`.

---

## (1) Functions that need a conversation id instead of a contact index
- **Lookup:** `conv_of`, `conv_mut_of`, `active_contact`, `dm_conversation`, `RowView::of_contact`, `strip_target` (and `selected_msg`/`pending_delete`, which key on `ContactId`), `open_conversation_with`, `clear_unread`, `scroll_to_message`, `compose_ready`, `lane_transmit_capable`.
- **Send:** `send_chain_message`, `send_control` for Delete and Wave, `chain_transmit`, `chain_transmit_with` (split into a bond transmit and a conversation transmit), `route_for_contact` (→ routes for a conversation; generalise `routes_for_molecule`), `resend_held_messages`, `drain_pending_chain_sends` (and the `pending_chain_sends` key).
- **Persist and push:** `persist_messages_async`, `persist_messages_signalled` (fold into `persist_conversation_async`), `push_rows_to_siblings` (→ `_token` with the stored token), `drain_persist_done`, `reserve_fleet_forwards`.
- **History and notify:** `kick_fleet_history_sweep`, `drive_history_recovery`, `summary_chirp_and_flag`, `sweep_undischarged`, `broadcast_focus_claim`, `save_messages_page` and `load_message_page_before` (their_seed → conversation id), the HistoryRequest serve route.
- **Attachments:** `attach_send_now`, `send_attach_blob`, `send_attach_chunks`, `attach_fetch`, `replicate_sweep`, `wave_export_start`, `holdings_start_pull_job`.
- **Waves:** `send_wave_signal`, `on_wave_signal`, `ring_alert`, `end_wave`/`drain_wave_keep` (`contact_index_by_handle_hash` picks *the* conversation for a peer), `derive_secret_for`.
- **Row actions:** the driver delete, star and resend arms; `clear_active_history`; `send_bond_offer`, `forget_offer`, `undo_invite`, `park_bond_offer` (they write offer rows into the pair conversation).
- **Fid → contact maps that must become fid → conversation:** `drain_braid_tx`, `commit_braid_rx`, the MessageAck arm, the Online-arm digest and re-serve, `rotated_flush`, `retransmit_due_messages`, and `retransmit_requests`.
- **Should stay keyed by contact or bond:** everything in `era.rs`, `ceremony.rs`, the CLUTCH arms, `maybe_send_chain_probe`, `seal_chain_if_ready`, `repose_clutch_round`, `rekey_without_chains`, `initiate_friend_rekey`, the sibling reset, blind ops, the knock, bridge and pigeon functions, and `spawn_wave_engine` addressing.

## (2) What wave and beam keys depend on in the pair chains
- **Wave secret** (`src/wave/keys.rs:50`): the inputs are `offer_lane_key ‖ wave_id ‖ nonces` only. The offer lane key is the lane key of **whichever chains carried the Offer row**: captured at send commit (`messaging.rs:712-742`) and at receive pre-advance (`conversation.rs:1567`). If the Offer stays a conversation row, the egg is a conversation lane key. If it moves to the bond, the egg is a bond lane key. Either works, but both ends must pick the same one.
- **Express key** (`src/wave/signal.rs:28`): `express_key(lane_root, history_key)` of the contact's `friendship_id` chains, current plus retired era (`wave_ui.rs:1214-1252`). The receiver trial-opens across **every** `friendship_chains` entry and maps a hit back to a non-sibling contact by fid (`wave_ui.rs:1384-1408`). After the split, decide whether the express key is the bond's or the conversation's, and make the fid → contact/conversation mapping work for both. Molecule chains already get trial-opened but never map to anything.
- **`derive_secret_for`** requires `contact.friendship_id` to exist (`wave_ui.rs:1497`).
- **Beams:** `beam_session.rs:169` `tx_key(wave_secret)` and `beam::start_rx(wave_secret)`, so they inherit the wave dependency and nothing more.
- **Recording spools** use a per-wave minted key; no chain dependency.

## (3) Wire and cross-device data that embeds the pair token or friendship id
Pair token in the `tok` field, all built by `src/network/fgtw/protocol.rs`:
- **Messages:** ChatMessage and MessageAck (signed provenance binds the token, `network/status.rs:5080/5089`).
- **CLUTCH:** offer, KEM response, complete. The token is inside the signed offer, so it feeds the offer provenance and therefore the `ceremony_id`.
- **Bond-type traffic:** friend_knock, chain_pull and miss (era_pull too), blind put/get/srv/ack.
- **History and attachments:** history request and page, attach blob/manifest/chunk/req/have/none.
- **Fleet:** chain_sync (outer token plus an inner check after decode), focus claim, chain_reset (sibling pid-pair token).
- **Sync records:** the token plus era index and tag inside the sealed pong tail, all records to every pinging device.

Not tokens, but related:
- Pigeon frames use `handle_hash` as the "token".
- Express wave frames carry no token (trial-open).
- `MoleculeWire` and the message package carry no friendship id.

Key material bound to the pair FriendshipId: `derive_history_key` and `derive_lane_root` (`clutch.rs:71/89`, via `from_clutch_woven`), and `derive_era_keys(fid)` (`era.rs:288`). The era transcript binds the token (`era.rs:263`).

Persisted or replicated:
- the contact state `friendship_id`;
- the chains blob's `friendship_id` and participants (the token is never stored);
- conversation table id = pair FriendshipId;
- `conv_state` vault key; draft keys (conversation id);
- the fleet roster's `ceremony_owner` and `woven`.

## (4) Risks
1. **The token codec** (top of this report): bonds lose their token on reload and on chain_sync.
2. **One registry.** `friendship_chains` is searched by token or fid in about 30 places, and `contact.friendship_id` is the only 1:1 link. Adding bonds to it without a kind tag sends ACKs, the Online arm, the express trial-open and `update_sync_records` to the wrong half. The bond also publishes an empty (0) digest record.
3. **Group ledger bypass in the Online arm** (`status.rs:509-557`). `clear_pending_up_to`, `rearm_pending_after`, `our_lane_wedged_at_peer_anchor` and rotation run on **any one member's** tip, ignoring pending `targets`. This already affects molecules today, and it becomes the general case once every conversation is roster-backed.
4. **The fork detector and repairs conflate the halves.** A conversation garbage streak triggers `initiate_friend_rekey` (re-CLUTCH), and the 256-row conversation cadence triggers the *pair* light ratchet. `reclutch_chainless_contacts`, `repose_clutch_round` and `rekey_without_chains` null the shared `friendship_id`. After the split, conversation problems should lead to a mint or re-delivery, and bond problems to a bond re-key. Era rows that arrive on a conversation token must be dropped (doc §7).
5. **Ceremony completion supersedes the conversation's era today** (`ceremony.rs:1067-1091`), and the heavy weave reads its `EraPrior` from `claim_fid` = `contact.friendship_id`. Both must point at the bond, or a re-CLUTCH will silently replace the conversation keys.
6. **The pair `history_key` does three jobs**: friend history pages, the attachment wire key, and the express key. The bond is derived from that same era (`derive_bond_split`), so the doc's rule "never hand out an era ≤ `bond_split_era`" also covers these three uses. Joining the pair conversation must mint first.
7. **Storage page functions are pair-derived** (`save_messages_page` / `load_message_page_before(their_seed)`). The history serve route and the fleet route (`contact_idx_for_conversation_token`) cannot serve roster conversations, so molecule history is only replicated by live pushes.
8. **Notes token mismatch:** the self token is `derive_conversation_token([us, us])`, which does not dedup, while `Conversation::new` dedups the id. The migration must keep these exact bytes (`push_rows_to_siblings`, `contact_idx_for_conversation_token`, attach_fetch's handle_hash fallback).
9. **Contact-only capabilities with no molecule path:** strip actions (delete-for-everyone, resend, star), attachments, waves, focus claims, friend history walks, gap-triggered walks (excluded at `status.rs:1442`), and the ckpt/vault/holdings enumerations. Converting to conversation ids changes all of them at once.
10. **`Contact::molecule_view` sets `friendship_id = gid`**, so `friendship_id.is_some()` gates pass on the group view. Remove both together.
11. **Mixed versions.** Unupgraded peers keep ACKing on the pair token with `process_ack` (hash-matched). A converted pair conversation must keep accepting the pair ACK path, not only `process_group_ack`. Bond-token frames rely on contact admission at the RX worker; member bonds between never-friended parties need a `molecule_admits`-style door.
12. **Pong record growth and leakage.** Every pong carries every conversation's token record (plus bonds after the split) to every friend device. It is sealed per device, but it grows with N and tells each friend how many conversations you have.
13. **Hidden Probe, Era and Molecule-invite rows live in the pair conversation table** (`conversation.rs:1668-1750`, `molecules.rs:644`). Moving that traffic to the bond either orphans those rows or needs bond-side row storage. Offer cards are also rendered from those rows (`render.rs:168`).
