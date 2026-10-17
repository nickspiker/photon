---
name: project_history_early_stop_blindness
description: "2026-09-24 CONVICTED+FIXED: head-page early-stop hid divergences older than page one (peer_ahead fix); catch-up chirped per page (one summary now); per-row vault commits (rarangi::put_rows_in)"
metadata:
  type: project
---

Field 2026-09-24 (Nick's phone cacbc223, v103 self-update): 36 chirps in 3 min = a six-hour backlog of 288 rows landing at once.
It was a REAL hole, not spurious alerts.
Three fixes, all shipped:
1. **Early-stop blindness.** `merge_history_page` marked a walk complete when `was_complete_before && fresh.is_empty()`, and `kick_fleet_history_sweep` armed that from the conversation's own `complete` flag — so every sweep stopped on the head page and a hole older than page one was unreachable. Now `Conversation::peer_ahead` (set at the pong when any peer advertises more rows, cleared only by a page the peer calls last) disables early-stop.
2. **Chirp per page.** The catch-up summary fired per merged page; rows now accumulate unflagged in `HistoryRecovery::pending_alert` and fire ONE summary when the walk ends.
3. **Commit count.** `rarangi::put_row_in` cost two durable commits plus a whole-catalog re-encode per row; `rarangi::put_rows_in` (3ad7075) puts a page in ONE transaction (a test asserts the counter moves by one).

**Why:** commit COUNT is the vault's cost (a flush is ~900 ms on busy storage), and an early-stop rule without divergence evidence is blind by construction.
**How to apply:** any "stop early" rule must be disarmed by evidence the peer holds more; batch row writes per page. Related: [[project_history_recovery]], [[project_notifications_pinned]].
