---
name: project-backfill-dings
description: "2026-10-21 CONVICTED (Emma's new phone 9ce64cc2): friend re-serve of old rows over the LIVE path rang once per 81-day-old photon; fix = behind_held gate in conversation.rs; plus the other v122 field findings from that log"
metadata:
  type: project
---

**Conviction:** after v122 the new phone's history walk stalled (page request before 6 Aug kept expiring), so 31 Jul–6 Aug arrived only via the friend's SENDER-SIDE RE-SERVE (status.rs ~622) as ordinary live chat frames at original stamps. The live ding gate (`will_ding && is_new_row`, conversation.rs) had no notion of "old" — is_new_row is only "not held yet", which is every row on a new device. 8 chirps, one per ~30 s, oldest first. No NOTIFY log line exists on that path (only Kotlin `vibrateChirp`).

**Fix (2026-10-21, committed with the MacBook batch):** `behind_held` = a friend row older than one already held from the same author → silent, flagged notified, no unread bump; logs "NOTIFY: backfilled row behind a held one". A truly late row still rings (lane order: nothing newer lands ahead of a held frame); known miss = a late row from one friend device after a newer one from another of their devices.

**Other findings in the same log (not yet fixed):**
- v0.121 held a friend photon 20 h on `LANE: braid strand miss … holding this frame` (×2158) — v122 delivered it at once.
- PT `Hash mismatch` ×2958 when several friend devices sent 258-pkt transfers at once: the path-migration adopt fallback (pt/mod.rs ~519-545) splices one sender's packets into another's transfer → timeouts, 432 attach re-asks, probably the stalled history pages.
- Vault `SLOW commit` 3.3-5.7 s blocking the UI thread (status arm Online 2.1-2.3 s); one `kete: LIVE-LAP AVERTED at lba 19`.
- This device served the same 5-row page to the friend 607× (~every 90 s for 20 h) — friend-side re-request loop.
- `HISTORY: page … DROPPED — rid unmatched and sender is not a fold-trusted sibling` ×40 after update.
- Beam TX on that phone: 45 encoded / 898 dropped (~1.4 fps). `setBuffersDataSpace(BT2020|GAMMA2_2|FULL) = -22` on its panel.
- Ghost 1be949c1 FRIEND-REFUSE/UNREFUSE flapping ~41×; `Status: active device  unreachable` empty name.
- No permission state is ever logged (notification permission, battery-opt) — add one startup line.

**Nick's MacBook fe46a74b (same pull, 2026-10-20 21:19 → 10-21 00:54, pre-v122 build):**
- 13,227 Conversation renders ~1/s all night (18 ms median): LIVE DIGITS (driver.rs ~3581) repaints the selected row's age each second in Arabic/Hex base, not gated on the window being watched (the wave field is).
- Main loop ~114 wakes/s idle (open [[project_fluor_busy_loop]]).
- `drain beam_armed took 1572ms` = camera start on the UI thread (wave_ui toggle_beam → camera_avf); camera prompt mid-wave.
- History echo: 268 × "merged page (1 new)" each followed by a live push of 1 row and a no-op persist — suspected blank-twin fold counted as new (types/conversation.rs ~217, conversation.rs ~2634).
- `dev_for` (sync.rs ~441) yields an ALL-ZERO pubkey endpoint: pings/history to 00000000, "pong answered by c9b5f417 but we pinged 00000000", and the offline verdict rotates the active device every 90 s.
- Pixel cacbc223 dead all night (conn refused → relay pipe closed ×522) yet 338 transfers queued: "(0 unreachable)" counts only address-less siblings.
- One friend's expired temporary IPv6 address: 2,741 retransmits, serial per-address queue delays everything behind it.
- Ghost 1be949c1 still pinged every cycle (437 locked-out).
- Mac notifications go via osascript, unlogged; Dock reopen (fluor host/app.rs) never calls set_window_visible(true).
