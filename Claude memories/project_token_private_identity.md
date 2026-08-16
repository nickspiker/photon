---
name: ""
metadata: 
  node_type: memory
  originSessionId: 63c01259-bf21-462c-a8f7-66960ca42e2f
---

The TOKEN crux — a private identity "held by no other party in any form" that is nonetheless survivable with no central store — resolved 2026-07-09 in conversation (Nick's insight, sparked while reading TOKEN/patent/provisional.tex Brief Summary).

**The construction:**
- S = random user secret, user-controlled entropy at first-device genesis. NEVER handle-derived (the current all-deterministic-from-handle model stays as the PUBLIC rendezvous layer; S is the private ownership root — the TOKEN two-layer split).
- pad(D,F) = PIPE_D("secret" ‖ handle ‖ friend) — provably-lossy OWF over the device hardware secret (patent's binding formula); public constant in, device-keyed noise out; nothing stored, recomputable any session; context string varies → unlimited independent pads.
- blind(D,F) = S ⊕ pad(D,F), deposited with friend F, keyed (handle, device pubkey). Full-entropy pad ⇒ ONE-TIME PAD ⇒ the blind is information-theoretically independent of S — friends provably hold "not the secret in any form" (the exact patent clause).
- Recovery: friend serves the blind ONLY to a currently-attested fleet member (serve-gate rides the fleet chain + eagle-timestamp freshness rule in device-lifecycle.md §3). Device XORs with its own pad; S hot in RAM momentarily, zeroized after; never at rest anywhere.
- Multi-device: per-device pads, multiple blinds (NOT a fleet-shared pad). New device receives S once over the sealed pairing channel at ADD, then deposits its own blinds.
- Theft: thief has pad but friends refuse the serve to a removed device ⇒ theft of device ≠ theft of secret. Serve-gate is load-bearing ⇒ same dependency on device-remove as the re-key fixes ([[project_rekey_attack_surface]]).
- Total fleet loss: blinds are noise by design (no device, no pad); that's the seam for the custodian K-of-N tier ([[project_manifestus_custodes_split]], whakaira ceremony) — bit-identical S ⇒ recovery restores continuous state, not a new credential.

**What it buys photon now:** un-moots the MAC-in-ACK history-authentication (tag = spaghettify(message ‖ S) in ACKs, friends store tags beside rows, recovered rows failing recompute are rejected ⇒ history injection dies cryptographically, not just via revocation).

**Don't-fuck-it-up list:**
1. Raw XOR is malleable — wrap the blind in kete AEAD keyed BY the pad + cross-check reconstitution against ≥2 friends ("agreement among independent groups").
2. S never handle-derivable, ever.
3. Serve-gate needs device-remove to exist (theft story aspirational until fleet mgmt grows remove).
4. Hot-RAM discipline ([[project_secret_memory_hygiene]]); today's device_secret is a software-readable fingerprint hash — full "uncopiable" claim is what PIPE closes; photon-now = honest software approximation.
5. Per-friend AND per-device pads so colluding friends can't even correlate blinds.

**Rename falls out (Nick, same day):** with S as the root, the handle is a mutable LABEL — you can change it any time (new handle_proof PoW = anti-churn). Friends authenticate the rename by device-key/S continuity (the allowlist they already hold), re-point privately over existing E2E channels — no public old→new linkage needed (deadname-safe), and reputation binds to the credential not the handle, so renames neither launder nor leak. ENDGAME REFACTOR POINTER: friendship_id/conversation_token/vault roots are handle-seed-derived today and would all churn on rename — they should eventually anchor on an S-derived stable credential key (PK_S), demoting the handle to pure discovery/phonebook. Belongs with the messaging rework. Whole-circle/kid case: childhood identity likely = scoped DELEGATION from the parents' identity (patent's delegation clause); majority = kid generates their own S + parent-signed introduction carries the social graph; childhood record seals like juvenile records. Same-S-from-birth alternative has a hard limit: custodian parents who ever held a K-quorum of shares can forever reconstruct that S (Shamir shares can't be un-shared). Keep the handle OUT of the pad input (or re-deposit blinds at rename).

**IMPLEMENTATION (2026-07-09): phases 1+2 SHIPPED, E2E pending.**
- Phase 1 @ fc841f4 — FLEET WEAVE: full CLUTCH between own devices. sibling_party_id(device_pubkey) in contact.handle_hash of synthetic is_sibling Contacts (friend paths bit-identical, pid==seed); party-id seam across photon_app (~25 sites: drains shadow per-contact, token matches per-candidate, chat/ACK pick whichever id is a chains participant); state keyed by contact.handle_hash; sibling index at vault_key("siblings", vault_seed); reconcile_fleet_siblings on attest/fleet-event/Bound, own-hp fold routed to reconcile NEVER into fleet_members (self-contact would swallow sibling pongs via first-match knows_device); siblings hidden from UI/roster/LAN/history-recovery.
- Phase 2 @ f057c5d — S LIFECYCLE: crypto/blind.rs (pad/blob/check/s_id/PrivateS/sibling seal); blind_put/ack/get/srv frames (hist-frame idiom, parse_any_blind_frame, packet-acked BOTH RX branches, TX rides send_history); PROBE-BEFORE-GENERATE at weave-seal ([]n wipes local markers so the NETWORK is asked, not the disk); Provisional→Live only on blind_ack sent AFTER the friend's disk commit; drive_blind_ops tick driver; sibling S-transfer = same get/srv frames on the sibling token, kete-AEAD under sibling chains' history_key (NEVER chat messages — they persist to the conversation DB = S at rest); split-brain converges on lower s_id; []n/[]u drop S.
- 125 unit tests green. Desktop + the plugged Samsung flashed. E2E TO RUN: (1) fleet weave — pair a 2nd device via 23-words, watch "SIBLING: reconciled +1" + sibling CLUTCH → "chain woven" both sides; (2) S — weave a friend pair, watch probe→found=0→generated(provisional)→ack→"S: live (s_id=…)"; []n one side → re-key → "S: reconstituted from friend blind" with the SAME s_id (NOT "generated").
- Phase 3 (MAC-in-ACK) not started — plan in ~/.claude/plans/ish-sync-recovery-we-refactored-wirth.md.

Related: [[project_peers_are_fgtw]] (friends-as-infrastructure), [[project_device_identity_model]], [[project_token_terminology]], [[project_keyring_design]] (fleet weave = the braid-in foundation).

**THREAT-MODEL FINDINGS (2026-07-12 design session, not built):**
- **The handle is PUBLIC, not a secret.** `identity_seed = BLAKE3(x(handle))` is a CHEAP hash; friends type+store your handle to add you, so every friend can derive your identity_seed. Real secrets = `device_secret` (opens vault, NOT handle-derived) + S (authorship, friend-blinded). Airport-theft wound = the DEVICE (device_secret→vault + live session→S in RAM), NOT the handle-on-shirt. Boot-lock works because the session stores the identity_seed REGISTER not the handle string → rebooted stolen device can't re-derive identity_seed → bricks (needs no handle secrecy). Dev-build seed-log leak (handle_query.rs) is therefore LOW severity (identity_seed alone opens nothing) — hygiene, not urgent.
- **S-as-recovery-lever:** S reconstitution needs each friend to SERVE the blind; friends serve only to devices they trust. Identity reclaim after theft = each friend flips trust for the stolen device + owner re-mints S (new s_id epoch) → stolen device's held S becomes superseded epoch. Custodians BATCH this social-graph authority (O(friends)→O(1)); they never seize.
- **GAP (not built): friend-side trust has NO downward override — currently UN-STRANDABLE.** `knows_device` (types/contact.rs:408) is a pure function of the folded chain — friends auto-trust EVERY current chain member, no way to refuse one. With consent-only removal ([[project_device_loaners]]) a compromised member is unremovable AND un-strandable, and a compromised member can PLANT unremovable devices. FIX (must ship with consent-only): `effective_trust(dev) = folded_member(dev) AND NOT friend_locally_refused(dev)` — reversible, friend-controlled, routing-layer (chain stays pure). One knob neuters stolen+planted devices identically. Makes "friends refuse the serve" a REAL capability. See [[project_total_loss_recovery]].
