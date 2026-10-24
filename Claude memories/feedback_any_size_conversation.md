---
name: feedback-any-size-conversation
description: "HARD (Nick, repeated, 2026-10-24 angry): a conversation of ANY size — you alone, you+1, you+N — is treated IDENTICALLY; one model, one key ceremony, one UI; never separate 1:1 vs group machinery"
metadata:
  type: feedback
---

**The rule:** every conversation is the same object regardless of member count (1, 2, … 2^256). Same membership model, same key agreement (CLUTCH), same eras, same panel (Manage, Bonds, stats, crypto state), same status surfaces on the conversation screen. A 1:1 friendship is a two-member conversation; your notes are a one-member conversation; a group is just more members. Nick: "HOW MANY GODDAMN TIMES DO I HAVE TO SAY THAT 1-ANY SIZED CONVO IS TREATED IDENTICALLY!!!" (2026-10-24), after I explained that molecules don't run CLUTCH and so show no crypto steps.

**Why:** two code paths for the same concept is where every divergence bug lives (the molecule join couldn't complete because group membership rides friendship eras it has no view of); and the user must see the same thing everywhere.

**How to apply:**
- Never describe or build "friendship vs molecule" as different kinds. If code has the split (it does: friendship CLUTCH + eras vs molecule roster + KEM wraps, ContactPanel vs MoleculePanel), treat it as debt to converge, and say so — never present it as the design.
- Any new feature (crypto status, invites, Bonds, stats) lands for every conversation size at once, not per kind.
- There is NO singleton "notes" conversation: you can found as many you-only atoms as you like (Nick 2026-10-24). The old self-contact (search your own handle → a forced-Complete Contact) is the last singleton and folds into atoms.
- Related: [[project_fleet_routing_scale]] (the same invariant for fleets), [[feedback_self_is_a_contact]], [[project-molecules-build]].
