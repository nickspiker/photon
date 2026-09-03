# Ghost device supersession — design note (2026-09-02)

## The incident

The phone re-attested (its device id moved `1be949c1` → `cacbc223`), but the published fleet membership chain still carries `1be949c1`.
Every fleet fan-out (messages, chain-sync, pings, doorbell) therefore sprays a permanently dead leg: the 2026-09-02 field day logged 50+ relay drops to `1be949c1` in minutes, and stale rosters holding the ghost were a convicted contributor to the no-ring call incident (2026-08-29).

## Why it can't just be removed

Departure is BILATERAL by design: the leaver signs a request, a survivor countersigns (mirror of add; expulsion never).
The ghost cannot sign — its key material was superseded by the re-attest, and on the same hardware it may be literally gone.
And the doctrine holds: identity never dies, ostracism not erasure, the chain is testimony, not a routing table.

## Options

**A. Hardware-continuity supersession (chain-level, the real fix).**
Device identity is deterministic from the hardware fingerprint (tohu), so the successor CAN prove it is the same machine.
A supersession record: the NEW device signs "I supersede `1be949c1` — same hardware, new era", a surviving fleet member countersigns (the exact bilateral shape of departure, with hardware continuity standing in for the leaver's signature).
The ghost stays in the chain as history; it gains a superseded-by marker; routing and quorum math skip superseded members.
This belongs in the fleet-key redesign (docs/fleet-key.md, era handling) rather than as a bolt-on — the redesign already models eras.

**B. Routing ostracism (interim, no chain surgery).**
Fan-out skips a chain member when (a) another attested device claims the same hardware lineage, or more weakly (b) the member has had no roster address AND no signed traffic for the whole session while a sibling on the same handle answers.
Doctrine-compliant (verify-or-withhold; nothing erased), zero chain risk, reversible the moment the ghost ever signs again.
Cost: heuristics; the ghost still occupies a membership slot in device-count displays and epoch math.

**C. Do nothing until fleet-key redesign ships.**
Every spray at the ghost burns retransmit ladders and (pre-PT-retarget) delayed real traffic; post-retarget the cost is smaller but still nonzero (relay drops, doorbell rings, count-based UI lying by one).

## Recommendation

B now (small, reversible, kills the daily bleeding), A as a designed part of the fleet-key redesign (it needs the same era machinery anyway).
Not built — awaiting Nick's call.
