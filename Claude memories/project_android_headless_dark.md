---
name: project_android_headless_dark
description: CONVICTED 2026-10-13 (Brittany, 0.110.10): an Android phone goes DARK in the background — Activity destroy frees the whole Rust app (nativeDestroy) so the service's ticks are no-ops, and a sticky service restart has no fingerprint so the network never inits; FCM doorbell wakes poke nothing. Plus Nick's desktop stale fold of her device and a Nick/Brittany era split.
metadata:
  type: project
---

**Field, 2026-10-13:** Brittany "never closed Photon, never unattested, never rebooted" yet looked offline for days; a photo Fetch did nothing.

**Convicted from her log (dev c9b5f417, 0.110.10) + Nick's phone (cacbc223) + Nick's desktop:**
1. **Dark in the background.** Before she opened the app at 19:37 UTC her log holds ONLY `PhotonFCM: Doorbell wake — poking protocol tick` every ~30 min; at 19:37 the network stack initialised from scratch. Nick's phone at 19:36: `RELAY: DROPPED — recipient's pipe is closed`. Mechanism (code): `PhotonActivity.onDestroy` → `setActivityContextPtr(0)` + `nativeDestroy` frees the Rust app; `PhotonConnectionService.requestServiceTick` returns at once when `activityContextPtr == 0` → no pings, no pipe upkeep, no CLUTCH. And `onStartCommand` inits the network only when the intent carries `fingerprint`/`dataDir`; a START_STICKY restart delivers a null intent → `networkPtr` stays 0 forever. The FCM wake handler only calls `requestServiceTick` (no-op). The memory "full Rust app lives in the foreground service" is FALSE as built: the app context is the Activity's.
   FIX BUILT 2026-10-13 (docs/background-tick.md "The app outlives the screen"): service owns `sharedContext`, screen adopts/lets go, `nativeCreateHeadless` on service start / FCM wake / tick-with-no-app, null-intent restart recomputes fingerprint+dirs, `headless_tick` = resume + protocol, WATCHDOG on the 1 s poll (PROTOCOL_BEAT; 15 polls warn, 300 exit → sticky restart). Field verify pending. Original plan: the service owns a headless app context when no Activity exists (create on service start / FCM wake, hand to the Activity on bind, never free it on Activity destroy); on a null-intent restart recompute fingerprint (DeviceFingerprint) + dataDir/shadowDir in the service.
2. **Era split Nick ↔ Brittany.** Nick's phone: `CHAT: frame tagged era 3140306098 from c9b5f417 — ours a0358af1 … dropped`. A fresh CLUTCH offer from Nick's phone was accepted by hers at 19:37:36 ("peer lost chains, accepting re-key") — expected to heal; verify `secret minted` on both.
3. **Photo fetch:** her REPLICATE asked Nick's desktop 90e571bf first; the desktop answered `ATTACH: request from unknown device — ignored` and separately `ping from unknown device c9b5f417 dropped — no contact's fold contains it` = Nick's desktop's fold of Brittany's fleet lacks her phone (stale fold). Chunks that DID arrive (from another of Nick's devices) failed `decrypt failed under both XChaCha and legacy ChaCha formats` — sealed under an era key she lacked (item 2).

Related: [[project_doorbell]], [[project_android_session_capsule]], [[project_pigeon_fetch_reach]].
