---
name: project-molecules-build
description: MOLECULES (was "groups", renamed 2026-09-15: you create an ATOM, binding a second person makes a MOLECULE; friendships stay distinct) PHASE 1 BUILT 2026-09-15 (plan steps 1–6, 8–10; v0.97.10 Android dev published); open = live strand pull (step 7), never-friended receive commit, sponsor offer refresh, Withdraw pill, phase 2 carriers
metadata:
  type: project
---

Groups (docs/groups.md §10–§11) phase 1 landed 2026-09-15 in seven commits after Nick approved the plan (plan file: so-consent-goes-with-joyful-adleman.md) and the Android dev build v0.97.10 went out with it (rebased onto the sixteen-language line; the thirty-seven group arms exist in all sixteen catalogs).

Settled at planning (the doc's D10–D14 + the A5 table): secrets only ever move as WRAPS to a published per-device KEM bundle (offer = snapshot, join = record + bundle, wrap = the era whole to a joiner / `fresh` to existing members; bundle id matched BEFORE decapsulation — the HQC decapsulator ABORTS on a foreign key, learned the hard way in a test); revocation = VOUCH AND WITHDRAW (standing derived; every vouch record kept; a departed voucher's late withdrawal erases nothing; Withdraw pill = phase 1.5); MoleculeLocal {muted, phase, offered} beside the roster, never in it; colour = group_id ‖ author.

What exists: roster v2 (title/bundle/vouch records), group rows in the ONE Ready list, the group conversation thru a transient `Contact::molecule_view` (never in `contacts`), founding from a friend's Manage page, the offer card + Join pill (msg_action slot 13; the block is 14 wide now), fan-out send (Route/Vec<Route> on BraidTxEncrypted; transmit_core shared with friendships; molecule_transmit; drain_molecule_posts frame-fenced), the per-member ACK ledger on PendingMessage (targets/acked_by, group blob only), MessageAck carries sender_pubkey, token-scoped RX admission (`status::set_molecule_devices` / `molecule_admits`), MoleculePeer fold, the roster riding the chains blob (`molecule_roster` bytes) so replication boots siblings, group rows pushed under the group token, the group panel (MoleculePage::{About,Add,Manage}, AppState::MoleculePanel), Leave (record last on our lane, root zeroized on its first ACK), eras (mint on lost standing + cadence by the lowest standing pid; minter cutover on first wrap ACK; Catching up), Settings → Conversations.

**Why:** Nick's ruling chain 2026-09-15: "a group is a friendship with more people", no owner, no registry name ("every group can be called taco"), consent goes with the device owner, banning = revoke needs every branch walked → A5.

**How to apply:** next work = (1) step 7 live strand pull (groups send ANCHOR-ONLY until then — never flip `anchor_only` for groups without it); (2) the never-friended receive commit (commit_braid_rx is contact-indexed ~42 sites — needs a peer-view resolution, NOT a group-peer Contact row); (3) sponsor re-offer on roster edges from MoleculeLocal.offered; (4) fractional delivered ring. Field test order in the plan's Verification section. Never re-serve a MOLECULE_PREFIX row bare (its payload rode the package only).


**Renamed 2026-09-15 (Nick: "Groups is not it"):** the object is a conversation you found ALONE — an atom; binding a second person makes it a molecule (H → H₂); friendships stay a distinct thing; the contacts screen filters All/Friends/Atoms/Molecules and founds atoms ("New atom"); an atom's panel offers "Create a molecule!" (= the Bind page); the invitee's pill is Bind; the code, storage domains, wire prefix (`photon-molecule`), hash domains (`PHOTON_MOLECULE_*`), settings key (`molecule.history_from_genesis`) and all sixteen catalogs say molecule/atom/bond. No field data existed under the old names. Never use "group" in new code or prose.
