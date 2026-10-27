---
name: project_sticky_catching_up
description: 2026-10-27 Petunia (Brittany's molecule): Nick + Emma stuck "Catching up" — CatchingUp is sticky (set by any unreadable-era frame, cleared only by a cutover); Brittany's device sends on an era nobody else holds
metadata:
  type: project
---

Field 2026-10-27 02:34–02:38 UTC, v131, molecule 2a1300b7 "Petunia" created by Brittany (contact 5fb63507, device c9b5f417); Emma = contact 8d2d1b2b, device 9ce64cc2 (confirmed from her log; she joined at 02:36:48 on v129 mid-update, restarted into v131 at 02:36:55, holds era #2 — the from-join mint for her).

Nick's fleet (Pixel joined, desktop + MacBook booted it from replication) held era #1 414f2df8, then cut over to #2 e6206a96 on 9ce64cc2's first frame (02:36:50.9).
Brittany's c9b5f417 kept sending on era a5dbc002 (2782642178) — held by no device of Nick's; every era_pull missed. Likely the group's genesis era (a from-join joiner never holds it) or a third era whose wrap never reached; needs Brittany's log (none submitted).

Defect 1 (convicted, desktop log): `catching_up` (status.rs, end of check_status_updates) flips Standing → CatchingUp for any frame on an era we don't hold, and only `after_molecule_cutover` clears it. Same batch: the #2 cutover set Standing, then the end-of-batch loop re-flagged it from Brittany's frame; no later cutover ever comes, so the header says Catching up forever while rows decrypt fine.
Defect 2 (open): a member's device sending on an era the joiners never got — her rows are held and replayed, then dropped again, so Brittany's photons likely never show for Nick or Emma.

**Why:** the header lies, and the group splits by era.
**How to apply:** CatchingUp should mean "a frame is held that our newest era cannot open and is newer than ours" — clear it when the held frames drain or when we hold the newest era any member advertises; and the sender side needs a fix (a device still on a pre-join era must cut over / re-deliver). See [[project_conversation_convergence]] (step F: eras over bonds).
Emma's log ends 02:37:20 with no unreadable-era frame yet, so her stuck label is unconfirmed but likely the same edge. Alternative read of a5dbc002: Brittany's device re-serving rows written before anyone joined (era #0) to the new from-join members, who can never open them — under from-join those rows should never be sent to joiners at all.
CONVICTED with Brittany's log (2026-10-27): her phone is healthy — a5dbc002 is Petunia's era #0 (genesis). She minted era 1 for Nick's join at 02:35:59.6, committed a row at 02:36:00.485 while still on #0, and cut over #0→#1 at 02:36:00.879 ("1 pending re-serve on the fresh lane" — the row was re-sent on #1 and delivered). The #0 copy had already reached Nick's devices, who joined from-join and can never open #0: it is HELD (molecule_held), REPLAYED on every roster advance ("held frame(s) replayed — the group moved"), dropped again, re-held, and each drop re-flags CatchingUp. Her later photons (02:36:51, 02:46:34) were ACKed complete by Nick and Emma.
Fix shape: (1) receiver — a held unknown-era frame stamped before our current era / our join is history from-join will never give us: drop it, never flag; a replayed frame that still fails never re-flags; clear CatchingUp when the hold queue empties. (2) sender — between a mint and its cutover, a group row's route to the joiner waits for the fresh lane instead of going out on an era the joiner was never given; member relay must not hand a member frames on an era older than their join.
