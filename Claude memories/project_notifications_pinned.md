---
name: project-notifications-pinned
description: "Fleet-wide notifications BUILT 2026-08-18 (17e9459): per-row notified flag, one focus claim, fleet-attention ball, one 120 s recency guard at the arrival edge; 2026-10-10 poke clock (press/key/IME only)"
metadata:
  type: project
---

Design (Nick 2026-07-23 + 2026-08-18), all built:
- **Per-row `notified` flag** on the rārangi row (stored inverted as 'unnotified'), fleet-synced like `delivered`, never seen by friends. It records the fleet discharging its alert duty once — NOT "unread".
- **One focus claim fleet-wide:** the newest focus-gain/open wins, scoped to the open conversation; retracted by focus loss/screen off, voided by presence loss.
- **Fleet attention (the ball):** `fleet_attention` = (device, osc) of the human's newest input, LWW by eagle osc + device tie-break, Lamport-bumped; frames only on the transition edge. Both ding gates require holding attention. FRESH = poked within 120 s, checked ONLY at the message-arrival edge (Nick-approved sole time constant — the arriving message is the clock). A stale holder retracts at that edge; drop-sweeps fire on retraction, presence void, and attention displacing the claim holder.
- `will_ding = !looking && !claimed_elsewhere`, computed pre-insert so the flag rides the sibling push; exactly one device dings; backfilled notified rows stay silent; an unflagged batch → one summary ding.
- The ding decision is post-decrypt; Android's Rust app lives in the foreground service; pause/resume latch as edge flags in nativeSetForeground, drained in tick.
- **Poke clock (2026-10-10, Nick: "has to be an actual tap or click … scrolling doesn't count, hovering doesn't count"):** the recency guard reads `last_poke`, stamped ONLY by a press, a key press or an IME commit (the set that takes the ball; the wheel dropped out too). `last_interaction` stays the presence-sweep clock. Focus gain takes the ball but is not a poke.

**Why:** exactly one device should ding, none while you're reading, and no timers beyond the one at the arrival edge.
**How to apply:** an input source joins the poke set only if it is a deliberate tap/key. Field verify pending. Related: [[project_history_early_stop_blindness]], [[project_doorbell]].
