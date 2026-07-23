# Background tick — advancing the protocol while the screen is off

> Status: **IMPLEMENTED** (Android phone + desktop). Verified: with the app backgrounded and screen on, `Status: pinged` and the full protocol drain keep firing on cadence — before the fix they stopped dead at `onPause`.
> This is the near-term, no-FCM half of the reachability story; the FCM/wake-from-deep-Doze half is [reachability-doorbell.md](reachability-doorbell.md).

## The symptom

Observed: "the phone doesn't complete CLUTCH until it's on and being stared at," plus a burst of duplicate ACKs when it finally wakes.

The duplicate-ACK burst is **not a bug** — traced end-to-end in the logs, it is the sender (the desktop peer) correctly retransmitting an un-ACKed message on its exponential backoff (attempts 2–6 at +1s/+2s/+4s/+8s/+16s over 31s) while the phone was unreachable, then all six retransmits landing at once when the phone foregrounded and each being faithfully re-ACKed by the self-heal path (commit `5bcf2bd`). The mechanism works. The *reason the phone was unreachable for 31s* is the real bug, and it is the same one behind "won't complete CLUTCH until stared at."

## The one physical fact

On Android, `PhotonApp::tick()` — which drains the network channels and **advances the CLUTCH ceremony and the message chain** — runs *only* from the Choreographer frame callback (`PhotonActivity.doFrame` → `nativeDraw` → shell → `tick`).
`onPause` calls `Choreographer.removeFrameCallback`. So the instant the app backgrounds or the screen goes off, `tick` stops, and:

- Incoming CLUTCH offers/KEM/complete and chat frames sit **undrained** in the status channel.
- The chain never advances; ACKs never go out; the sender retransmits until the phone is foregrounded.

Critically — verified against the Kotlin lifecycle — on `onPause` the shell and `PhotonApp` are **NOT destroyed**; `nativePtr` stays valid and all state is live in memory. `nativeDestroy` fires only from `onDestroy` (a real teardown: swipe-away / OS reclaim). So for the reported symptom, **the state is intact and simply un-ticked.** Waking the tick is the whole fix; no state needs to move or be shared.

## Why not the lumis split-worker / shared-memory model

lumis runs the camera as an independent background producer (`jni_camera.rs`) writing a flat `[u64]` + image-plane arena (`shared_memory.rs`) that the UI (`jni_ui.rs`) reads; coordination is a bumped counter the consumer polls. Right *shape* — two JNI worlds, background producer independent of the draw loop — but wrong *fit* for photon's state:

- lumis shares **dumb POD data** (pixels, scalar params) producer→consumer. Neither side advances a shared stateful machine.
- photon's CLUTCH/chain state is a **stateful protocol machine** (ratchet keys, pending-message queue, gap buffer, ceremony slots, McEliece keypairs) that must be *mutated in receive-order*. It cannot be a `[u64]` arena, and the advance logic is thousands of lines bound to `PhotonApp`.

So we take the lumis *heartbeat idiom* (producer signals, consumer notices) but keep every ratchet mutation on the single thread that already owns it — we do **not** put `PhotonApp` behind a shared lock or copy it into an arena.

The "split worker" photon needs **already exists**: `PhotonConnectionService` + its status RX thread is the background producer (it already runs while paused — it posts the message notification via the `MESSAGE_NOTIFIER` JNI upcall from `5bcf2bd`). What is missing is not a worker but the worker's ability to **drive a drain/advance pass** on the paused-but-alive `PhotonApp`.

## The fix: a headless service-driven tick

1. **Split `tick` into `advance_protocol()` + the render/animation remainder.**
   `advance_protocol()` is the pure-state subset already present in `tick`: `check_status_updates`, `check_clutch_keygens` / `_kem_encaps` / `_ceremonies`, `spawn_next_pending_keygen`, `retransmit_due_messages`, and the handle-query / add-device / join / fleet-roster channel drains. **None of these touch the surface.** The animation blocks (attest wave, hourglass, blinkey) and `render()` stay in the frame-only path. Foreground `tick` = `advance_protocol()` + animation + render, exactly as today.

2. **New `nativeServiceTick(nativePtr)`** (Activity-context ptr, reused by the service): runs `advance_protocol()` **without** drawing. The service calls it when woken.

3. **Drive it periodically from the service's existing poll loop — this is the load-bearing part.** The service already runs a self-scheduled 1s `nativeNetworkPoll` on its own `HandlerThread` (`startNetworkPolling`), independent of the Choreographer. While `!PhotonActivity.inForeground`, that loop also calls `requestServiceTick()` → `nativeServiceTick`. A brief `PARTIAL_WAKE_LOCK` wraps the tick so the CPU is scheduled to run it.

   **Why periodic, not just RX-triggered (the verification lesson).** The first cut woke the tick only on inbound traffic (the RX worker upcalling on each `StatusUpdate`). That path fires correctly — but it's a chicken-and-egg: the presence *ping send* lives in `advance_protocol`, so once `tick` stops there's nothing sending pings, so no pongs arrive, so no RX-driven `StatusUpdate`, so nothing to react to. The whole network stack quiesced within ~5s of `onPause` (only the last in-flight pings' TIMEOUTs trailed out). The **send side must be kept alive**, which needs a *periodic* driver — the 1s poll loop. The RX upcall is kept as a low-latency supplement (react to a packet the instant it lands rather than up to 1s later), but the periodic drive is what actually closes the gap.

4. **Guard against the resume race.** Paused ⇒ the UI thread's frame callback is removed, so the service tick and the UI tick are not normally concurrent. An `AtomicBool ticking` on `PhotonContext` serialises the `onResume` overlap: `nativeDraw` holds it across the draw (`Relaxed` set, `Release` clear); `nativeServiceTick` CAS-acquires it (`Acquire`) and simply **skips** if a draw holds it — a dropped background tick is harmless because that draw drains the same channels. `PhotonApp` stays effectively single-threaded.

5. **Pointer handoff + teardown safety.** The Activity hands its native context ptr to the service (`setActivityContextPtr`) once the shell exists, and retracts it to `0` in `onDestroy` *before* `nativeDestroy` frees it — so the service never ticks a freed context. A `0` ptr on the service side is a no-op.

## Scope boundary

- **Fixes:** backgrounded / screen-off (app *paused*, state alive). This is the reported symptom.
- **Does NOT fix:** app *destroyed* (`onDestroy` ran — swipe-away / OS-reclaimed the process) — the state is gone and must be rebuilt on next launch; and **deep Doze**, where the OS won't schedule the service thread to run the tick at all regardless of wakelock. Those two are the [reachability-doorbell.md](reachability-doorbell.md) territory (FCM remote-wake). This doc is the cheaper tier that closes the common case without Google in the loop.

## Open question

Whether `nativeServiceTick` should run the *full* `advance_protocol()` or a narrower "just drain + ACK + advance chain/ceremony" subset while backgrounded (deferring, say, avatar downloads and roster merges to the next foreground tick) — to keep the backgrounded wake cheap. Start with the full pass; narrow only if the wake proves too heavy.
