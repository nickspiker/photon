---
name: project-groups-build
description: Groups implementation status — steps 1 + 2a–2d-ii SHIPPED as local commits (NO PUSH until Nick clears call tuning); next = the send side
metadata: 
  node_type: memory
  type: project
  originSessionId: 792ef7a0-4bc0-434e-8067-5676343cc470
  modified: 2026-09-14T19:21:32.856Z
---

Groups (docs/groups.md, §8b settled decisions) build state as of 2026-09-14. **ALL LOCAL COMMITS, NOT PUSHED** — Nick has concurrent call-tuning work; ask before any push.

Shipped (9 commits, 38058175 → 074d0c4a, all tests green incl. full-suite gate):
- Step 1: types/group.rs (GroupId/token/founder_scoped_proof, sovereign Roster merge, GroupPeer, sign/verify), ChatMessage.author thru all four codecs (m_aun/m_auv page columns), chains blob v9 (ADDITIVE, group-only — friendship blobs stay byte-identical v8, pinned by test), FriendshipChains::from_group_root (no ceremony), EraDecapKeys in crypto/era.rs (persisted group KEM custody, export_decaps/era_decapsulate_group).
- 2a: roster custody at vault_key("group", gid) — roster codec is ALSO the wire record envelope (a partial roster = a record posting); found_group → GroupBirth.
- 2b: GroupSignal Invite/Records control rows (GROUP_PREFIX, EraSignal shape); message package `gpl` typed blob field.
- 2c: GroupWire {from, woven_authors, blob} on the package (gfrom presence = group frame; gwa pairs 1:1 with wt, mismatch drops authors only); StrandLedger (offer/seed/holds, pull-from-sender, unpark cascade) with the §9 DAG test (3 members × 3 delivery orders converge).
- 2d-i: group_list index at vault_key("groups", vault_seed) (the flat vault enumerates NOTHING — the doc's §7 missed this), load_all_groups, boot passes in driver.rs resume + handle_query attest worker (AttestationData.groups), PhotonApp.group_rosters.
- 2d-ii: receive path — both token scans branch on chains.group; sender = standing member's folded device (v1 thru Contact fold; GroupPeer fold = step 3); pkg.group.from must agree with transport verdict; msg.author fed; conv resolves by minted id; group tokens excluded from rekey_probe + friend-history walk; ui/photon_app/groups.rs on_group_signal (invite adopts LOCALLY — consent = our member record posts on FIRST SEND in the group; records merge signature-checked under roster law).

NEXT (2d-iii, the send side): chain_transmit is contact-indexed (peer_addr/recipient_pubkey from the contact) — a group send must encrypt ONCE on our lane then fan the same ciphertext to every standing member's devices. Pieces: found-group UI action + invite send (control row on the sponsor↔invitee friendship carrying GroupSignal::Invite text + GroupWire{from: our pid, blob: roster snapshot}); group text send (GroupWire from/woven_authors, prepend join_record Records row on our first send); per-member ACK ledger (§8b: pending keeps any-ACK bool, per-member set in sender's ledger). Then step 3 eras, step 4 UI.

Known deferred: chains.participants stale on roster change for groups (informational only); tier-1 network-thread allowlist (network/status.rs:3305 contacts_recv) blocks never-friended members — needs group-device union or token-scoped bypass, with GroupPeer fold in step 3; strand-miss parking wiring into the live gap buffer.

Also fixed en route: census_sweep_auto_nukes_strays raced log_dir()'s one-shot OLD_LOG_SWEPT Once (first log-sink open deletes the config-dir log) — test now spends the Once before planting its log.
