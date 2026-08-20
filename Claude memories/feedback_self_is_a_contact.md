---
name: feedback-self-is-a-contact
description: "HARD RULE, repeatedly violated: SELF AND BOB ARE BOTH PEOPLE — the self/fleet conversation is an ORDINARY conversation on the identical machinery; never special-case zero-remote/notes-to-self; fixes are SUBTRACTION of branches, never a 'self-sync' feature"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: d83fbeaf-685c-4da4-8647-7b49de82fd2c
---

**Nick, 2026-08-20, after saying it many times before: "self and bob are both people." "Everyone should be treated equal, I'm not special."**

The self conversation (notes-to-self, the fleet table) is an ORDINARY conversation. Your own devices are peers exactly like a friend's devices. Same rows, same lanes, same history walks, same anti-entropy, same ring tiers, same blob rules, same everything — the ONLY legitimate difference is which key material seals a pair (device-pair sibling secrets vs friendship secrets), and that difference lives at the key layer, never above it.

**Why:** every special case is a fork where one path rots unwatched. The proof: 17 `has_remote`/`is_zero_remote` branches accumulated, and notes-to-self silently stopped syncing (13 rows on one device, 0 on another, UI claiming "synchronized") while a drop site literally logged "self-sync black-hole suspect" — a session KNEW and shipped the suspicion instead of deleting the fork.

**How to apply:**
- Never write `if has_remote` / `is_zero_remote` / "notes-to-self" branches for behavior. If a path needs "the peer" and self seems not to have one — it does: the sibling devices. Route thru the same machinery.
- The standing fix direction is SUBTRACTION: delete the 17 branches, make the fold-trusted-sibling path serve the self conversation like any other, and the black hole closes as a side effect. There is no "self-sync feature" to build.
- When a genuinely unavoidable asymmetry appears (e.g., a conversation with literally one device and zero counterparts), it is a DEGENERATE CASE of the general path (empty peer set), not a separate branch.

Related: [[project-rarangi-messages-fleet]] (fleet = a conversation; vaults byte-identical except device crypt key — the doctrine was already recorded there and sessions special-cased anyway).
