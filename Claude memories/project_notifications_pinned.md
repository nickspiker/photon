---
name: project-notifications-pinned
description: "notification design (focus-claim + per-row unnotified flag, zero timers) BUILT 2026-08-18 @ 17e9459 — gate cleared by the ten-round soak; field verify pending"
metadata: 
  node_type: memory
  type: project
  originSessionId: f4272721-c713-4a82-a97a-db8106029756
---

USER'S DESIGN (final, 2026-07-23 — supersedes my beacon sketch; do not build until fleet sync + message recovery are tested and the braid-in bug is fixed):
- Per-message **unnotified** flag on the rārangi row, synced fleet-internally like `delivered` (fleet-key-sealed; friends NEVER see it). Explicitly NOT "unread" — notification state records the fleet discharging its alert duty once, never whether the human read (read-state-as-data sidestepped entirely; privacy concern deferred by construction).
- **One active-clearer fleet-wide**: the most recent focus-gain/instance-open event wins the role (a new instance's claim DISPLACES the previous device even if its window still sits OS-focused). Claim scoped to the conversation whose screen is open. Claim refreshed by interaction events (click/keystroke), retracted by focus-loss/screen-off events, voided by presence loss (existing sibling ping machinery — covers crash/sleep without timers).
- On friend-message receipt: if the active-clearer has THAT convo open + screen on → it clears the flag, broadcasts to the fleet, NOTHING dings anywhere. Otherwise → normal notify path; exactly one device dings, flips notified=true, broadcasts (true-wins, idempotent). Siblings decide locally against the already-known claim — no waiting on the clear broadcast, no race timer.
- Catch-up: a backfilling device sees notified rows → silent (kills wake-from-doze ding-storms); a batch nobody flagged (fleet was offline) → one summary ding, then flag.
- **Accepted quirk (user-stated)**: convo screen left open + screen on + human absent = that friend is silent all night, fleet-wide; other friends' convos still notify. The remedy is social, not a timer.
- Implementation merge-point: the ding decision moves POST-decrypt (rows exist there; probes/system already filtered) — same relocation that fixes the everything-dings bug (status.rs ~2027 RX-worker pre-decrypt ding becomes the cold-FCM-only fallback). Android: full Rust app lives in the foreground service, so post-decrypt logic is available with the Activity dead.
- Transport symmetry: friend's fleet already reply-TXes to the last-RX'd device — the notifier/clearer is naturally the device the conversation is concentrated at.

**BUILT IN FULL 2026-08-18 @ 17e9459** (gate cleared by round-10 soak): ChatMessage.notified (monotone, rides rarangi rows inverted-as-'unnotified', m_ntf page column, sibling pushes; friend pages force true), 'focus' sibling frame (claim/retract, newest-osc-wins, offline-verdict void), will_ding = !looking && !claimed_elsewhere computed PRE-insert so the flag rides the push, catch-up summary (one ding per undischarged batch). Field verify: two devices, one watching a convo → other device silent on friend msg; nobody watching → one ding; wake-from-doze backfill → single summary. ALSO deleted the expired v52 pre-document chains migration per its own gate.

INTERIM SHIPPED 2026-07-25 (@165d765, single-device subset of the design): notify + chirp fire exactly when NOT `looking` = (sender's conversation open AND app/window attended); Rust is the one suppression decision (Kotlin's blanket inForeground bail removed, onResume/onPause mirrored to Rust via nativeSetForeground). Post-decrypt relocation from the design is DONE (pre-decrypt RX ding removed earlier). Still missing = the fleet-wide parts: unnotified row flag, one-active-clearer claim, exactly-one-device-dings, catch-up summary.

Related: [[project-doorbell]], [[project-chain-advance-desync]] (braid-in fix is the gate), [[project-fleet-braid-plane]].
