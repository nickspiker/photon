---
name: project-device-sovereignty
description: "The ownership rule for ALL records — the subject signs, others verify or withhold; pending expires instead of being deleted; chain entries are testimony, not authority"
metadata:
  type: project
---

Nick (2026-07-13): **"I own myself and my devices own themselves and I own my devices. Everything else flows from that."**
The three clauses map to the three signatures: identity co-sign on genesis, the device's consent-sig on its own records, the sponsor member-sig on adds. Owning a self-owning thing = sponsorship + provision, never control.

**The rule:** every record is signed by the party it is about, and only that party can mutate it. Everyone else has exactly two powers: verify, and withhold.
Corollaries:
- Add is bilateral; an unconsented bind is pending, never final.
- Removal is bilateral departure ([[self-only-removal]]); theft would require the device signing its own theft.
- Pending states carry freshness stamps and EXPIRE; no third-party deletion/GC (the worker never consumes a request — the author withdraws, or it lapses).
- Chain entries are TESTIMONY: an ocean-dropped device stays on the chain forever; security = the group withholds (re-key around it); the UI shows a local tombstone. Ostracism, not erasure.
- Total loss = custodian-gated chain SUPERSESSION ([[project_total_loss_recovery]]): it replaces, never edits.
- The FGTW device→owner index must claim on DEVICE-SIGNED evidence, not mere chain listing (virgin-pubkey squat).

**Why:** Nick frames devices as persons — a device can't be stolen because it would have to consent to the stealing.
**How to apply:** any "who deletes/edits X?" resolves to: the author mutates it, or it expires — never the other side.
Delete semantics (docs/vault-delete.md, 2026-09-08): delete = space reclamation; the security boundary is the boot-gated vault key ([[feedback_discard_not_shred]]).
