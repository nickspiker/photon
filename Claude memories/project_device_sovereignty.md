---
name: project-device-sovereignty
description: "The distilled ownership rule for ALL records — subject signs, others verify or withhold; expiry over deletion; testimony not authority"
metadata: 
  node_type: memory
  type: project
  originSessionId: d2067fd8-576f-40e0-987a-80c59aedb715
---

Distilled 2026-07-13 (words-ceremony rework discussion), generalizing the removal decision in [[project_device_loaners]].

**Canonical plain statement (user's own words): "I own myself and my devices own themselves and I own my devices. Everything else flows from that."**
The three clauses map to the three signatures: identity co-sign on genesis (I own myself), device consent-sig on its own records (devices own themselves), sponsor member-sig on adds (I own my devices).
Owning a self-owning thing = sponsorship + provision, never control — theft must break both ownerships at once; the ocean price is the same fact inverted (can't be forced to sign ⇒ can't be forced to resign).
User wants this as the opening of the pairing-v2 doc rewrite.

**The rule: every record is signed by the party it's about, and only that party can mutate it. Everyone else has exactly two powers: verify, and withhold.**
No verb exists by which one party edits another's standing — only verbs of self (request, consent, resign) and verbs of group (verify, include-in-next-key, exclude-from-next-key).

Corollaries:
- Add = bilateral (sponsor sponsors + device consent-signs); an unconsented bind is pending, never final — retires pairing-v2's "sweep imposter ops" (nothing completed, nothing to sweep).
- Remove = self-signed departure ONLY (already decided in [[project_device_loaners]]); theft would require the device signing its own theft. DECIDED 2026-07-13: the chain flips to this NOW, ahead of the withholding layer — interim eviction = fleet-key subset rotation only, remove-other UI retires, lost test devices pruned by re-genesis.
- Pending states (binding requests, half-completed bilateral adds) carry freshness stamps and EXPIRE; no third-party deletion/GC ever (worker never consumes a request — author withdraws on green, or it lapses).
- Chain entries are TESTIMONY not AUTHORITY: an ocean-dropped device stays on the chain forever (true fact: consented in, never left); security = the group withholds (re-key S/friendships around it, demoted fleet key per [[project_device_loaners]]); UI shows a local tombstone. Ostracism, not erasure.
- Escape hatch at the extreme = custodian-gated chain SUPERSESSION ([[project_total_loss_recovery]]) — consistent because it replaces, never edits.
- FGTW worker device→owner index must claim on DEVICE-SIGNED evidence (not on mere chain listing — today fgtw-bootstrap lib.rs ~2698 claims every folded member, which enables a virgin-pubkey squat) and release on self-departure/supersession.

**Why:** the user framed devices as persons ("device itself makes changes, think of it like a person") — can't be stolen because it would have to consent to the stealing; the accepted cost is a permanently attached ledger entry for a lost device.
**How to apply:** any lifecycle question ("who deletes/edits X?") resolves to: the author mutates it, or it expires; never the other side. Use as the yardstick for the words-ceremony rework ([[project_pairing_v2]]), device-remove/re-key bundle ([[project_rekey_attack_surface]]), and any new WAN-hosted record.
