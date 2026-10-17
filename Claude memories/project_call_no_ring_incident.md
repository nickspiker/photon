---
name: project-call-no-ring-incident
description: "Wave no-ring / one-way / instant-drop class (2026-08-29 → 09-11), all convicted + fixed; the rules: siblings never destroy a wave, waves survive one era of skew, express sources must be real remote addresses"
metadata:
  type: project
---

Field incidents Aug 29 – Sep 11 2026 (Emma, Esme, Brittany ↔ Nick), each convicted from logs and fixed. The rules they left:

1. **Era-split friendship, not transport:** "messages/waves silently ignored + parked ceremony + doorbell loop + HISTORY rejected not-mutual" = an era split (2026-08-29 root: Nick's phone re-attested with a new device id because ANDROID_ID changed; the ghost 1be949c1 is the ocean-dropped old phone). Reconcile by re-clutch ([[re-clutch-never-store]]); never silently discard "No friendship found".
2. **Siblings observe, never destroy:** a stray answer at a device that never dialed is IGNORED (`dialed_wave_ids`); `send_express_signal` uses `validated_path` only as a fallback when the wave's express address is unknown.
3. **State a commit capture writes into must exist before the send** (the outgoing ActiveWave is created first; an offer committing with no matching outgoing wave logs loudly).
4. **A wave survives one era of skew either way:** `express_key` = the era key that opened the first express frame; replies seal under it first, offers under current AND retired eras.
5. **An express frame's source is a route only if it is a real remote address:** express targets and the media re-point refuse loopback/unspecified; anchor-for-anchor at most once per second (`last_anchor_osc`). Symmetric keys make "from <friend>" a claim about the KEY, not the sender.
6. **Drought clocks baseline on the UI thread before the engine spawns** (a stale previous-wave stamp dropped a wave 1 ms after answer).
7. **No direct path → relay every express frame** to `relay_device_list()`; the 3 s ring lease applies only after an express beat (`express_beats`), else 20 s.
8. **Android incoming:** notification Answer is an ACTIVITY PendingIntent (Android 10+ drops background activity starts); full-screen intent sets showWhenLocked/turnScreenOn; proximity wake lock held for the wave.

Open: lane head-of-line blocking of an offer row behind unacked texts; a re-clutch blanks the live friendship for the ceremony's length (eased by [[project_era_ratchet]]). Related: [[project_clutch_completion_rebroadcast]], [[project_clutch_token_asymmetry]].
