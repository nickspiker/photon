---
name: project_sticky_catching_up
description: 2026-10-27 Petunia (Brittany's molecule): Nick + Esme stuck "Catching up" — CatchingUp is sticky (set by any unreadable-era frame, cleared only by a cutover); Brittany's device sends on an era nobody else holds
metadata:
  type: project
---

Field 2026-10-27 02:34–02:38 UTC, v131, molecule 2a1300b7 "Petunia" created by Brittany (contact 5fb63507, device c9b5f417); Esme is probably contact 8d2d1b2b (device 9ce64cc2, on Nick's LAN; not in the pseudonym map).

Nick's fleet (Pixel joined, desktop + MacBook booted it from replication) held era #1 414f2df8, then cut over to #2 e6206a96 on 9ce64cc2's first frame (02:36:50.9).
Brittany's c9b5f417 kept sending on era a5dbc002 (2782642178) — held by no device of Nick's; every era_pull missed. Likely the group's genesis era (a from-join joiner never holds it) or a third era whose wrap never reached; needs Brittany's log (none submitted).

Defect 1 (convicted, desktop log): `catching_up` (status.rs, end of check_status_updates) flips Standing → CatchingUp for any frame on an era we don't hold, and only `after_molecule_cutover` clears it. Same batch: the #2 cutover set Standing, then the end-of-batch loop re-flagged it from Brittany's frame; no later cutover ever comes, so the header says Catching up forever while rows decrypt fine.
Defect 2 (open): a member's device sending on an era the joiners never got — her rows are held and replayed, then dropped again, so Brittany's photons likely never show for Nick or Esme.

**Why:** the header lies, and the group splits by era.
**How to apply:** CatchingUp should mean "a frame is held that our newest era cannot open and is newer than ours" — clear it when the held frames drain or when we hold the newest era any member advertises; and the sender side needs a fix (a device still on a pre-join era must cut over / re-deliver). See [[project_conversation_convergence]] (step F: eras over bonds).
