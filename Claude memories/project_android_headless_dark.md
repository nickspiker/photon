---
name: project_android_headless_dark
description: CONVICTED 2026-10-13 (Brittany, 0.110.10) — Android went dark in the background because Activity destroy freed the whole Rust app; fix BUILT (service owns the app + watchdog, docs/background-tick.md), field verify pending
metadata:
  type: project
---

**Field 2026-10-13:** Brittany "never closed Photon" yet looked offline for days; her log held only `PhotonFCM: Doorbell wake — poking protocol tick` every ~30 min until she opened the app.

**Mechanism:** `PhotonActivity.onDestroy` → `setActivityContextPtr(0)` + `nativeDestroy` freed the Rust app, so `PhotonConnectionService.requestServiceTick` (and the FCM wake) were no-ops; a START_STICKY restart delivers a null intent with no fingerprint/dataDir, so `networkPtr` stayed 0 forever.
The older belief "the full Rust app lives in the foreground service" was FALSE as built.

**Fix BUILT 2026-10-13** (docs/background-tick.md "The app outlives the screen"): the service owns `sharedContext` and the screen adopts/lets go; `nativeCreateHeadless` on service start / FCM wake / tick-with-no-app; a null-intent restart recomputes fingerprint + dirs; `headless_tick` = resume + protocol; WATCHDOG on the 1 s poll (PROTOCOL_BEAT: 15 polls warn, 300 exit → sticky restart). Field verify pending.

**Side findings same day:** a Nick↔Brittany era split (a fresh CLUTCH was expected to heal it), and Nick's desktop held a stale fold of her fleet (`ping from unknown device … dropped`, `ATTACH: request from unknown device — ignored`), so her photo fetch failed.

**How to apply:** an Android device "offline for days while running" → check whether its log shows only doorbell pokes (no network init) before blaming transport.
Related: [[project_doorbell]], [[project_android_session_capsule]], [[project_pigeon_fetch_reach]].
