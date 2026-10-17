---
name: feedback-self-is-a-contact
description: "HARD RULE, repeatedly violated: SELF AND BOB ARE BOTH PEOPLE — the self/fleet conversation runs on the identical machinery; never special-case notes-to-self; fixes are SUBTRACTION of branches"
metadata:
  type: feedback
---

Nick 2026-08-20, after saying it many times: "self and bob are both people." "Everyone should be treated equal, I'm not special."

The self conversation (notes-to-self, the fleet table) is ORDINARY: your own devices are peers exactly like a friend's — same rows, lanes, history walks, anti-entropy, ring tiers, blob rules. The ONLY legitimate difference is which key material seals a pair (sibling secrets vs friendship secrets), and it lives at the key layer, never above.

**Why:** every special case is a fork that rots unwatched — 17 `has_remote`/`is_zero_remote` branches accumulated and notes-to-self silently stopped syncing while the UI claimed "synchronized".

**How to apply:**
- Never write `if has_remote` / `is_zero_remote` / notes-to-self branches for behaviour; if self seems to lack "the peer", it has one — the sibling devices.
- Fix direction is SUBTRACTION: delete self branches; there is no "self-sync feature" to build.
- Audited 2026-10-17: the ~28 surviving `has_remote`/`is_zero_remote` uses are NOT self branches — each decides by `remote_count(&us)` (participants minus us), i.e. the general rule evaluated on the participant set, with notes-to-self as its empty case (the 2026-08-20 fix converted the old 17 branches to exactly this). Leave them; flag only a NEW check that tests "is this the self row" instead of the participant set.
- A genuinely unavoidable asymmetry (one device, zero counterparts) is a DEGENERATE CASE of the general path (empty peer set), not a branch.
Related: [[project_rarangi_messages_fleet]].
