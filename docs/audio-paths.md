# Every audio path, every platform

State as of 2026-09-03. Photon makes sound in exactly six situations. This is all of them, what plays, which device path carries it, how loud, and what stops it.

Two rules run underneath everything:

- **Latency-first, echo-ours** (2026-08-19/20). Both live-call device paths deliberately refuse the vendor voice pipeline: capture is `VOICE_RECOGNITION` (raw fast-track), render is `USAGE_MEDIA` (fast mixer). That bought back an 80 ms buffer floor and cost us the hardware AEC, so echo control is ours — the PID duck, the predictive gate, and the learner. Do **not** attach `AcousticEchoCanceler` as a stopgap; effects kick capture off the fast path and undo the whole trade.
- **Stops, not dB.** A stop is ×2 amplitude — one bit-shift. Live wave playback runs 4 stops down (`call::OUTPUT_PAD_STOPS`, `>>4`).

---

## 1. Live wave audio (the conversation)

| | desktop (macOS/Linux/Windows) | Android | Redox |
|---|---|---|---|
| device | cpal in+out, own thread (streams are `!Send`) | Kotlin `AudioRecord` + `AudioTrack` over JNI queues | none |
| capture | device rate → 48 kHz mono i16, linear resample | `VOICE_RECOGNITION`, raw fast-track | — |
| render | 48 kHz mono → device rate | `USAGE_MEDIA`, fast mixer | — |
| level | 4 stops down at rx enqueue | same (shared constant) | — |
| status | live | live | signaling only, `start()` returns false |

Engine ([`call/engine.rs`](../src/call/engine.rs)): mic → PID level+duck → Opus CELT on a 16→128 kbps AIMD ladder → RaptorQ window → sealed datagram; inbound reverses it. No PLC — a window that can't decode is silence, never guesswork.

**The 4-stop pad** is applied per rx frame just before `queue_playback`. Ritual prompts and recording preview enqueue **unpadded** (they're not the wave). The `RENDER_ENV`/`RENDER_REF` taps sit *downstream* of the pad, so the learner measures what actually left the speaker — expect measured `g` and `rx(play)` tallies ~4 stops below the pre-pad field waves.

**Android has no receiver path.** `USAGE_MEDIA` always routes to the loudspeaker at *media* volume; the earpiece receiver is only reachable via the communication path we traded away. The Kotlin route sniffer ranks *available* devices (earpiece outranks speaker), so logs and calibration-profile keys read `"earpiece"` while the physics are loudspeaker — a known mislabel, live in the field logs. The in-call speaker toggle is **parked** (handler, render, hit-stamp, widget walk all commented) because the fixed pad replaced it.

## 2. Ringback — we are waving someone ([`call/ringback.rs`](../src/call/ringback.rs))

The **callee's own ring** cadence, same relationship digest as their inbound ring, so the caller hears *who* they're waving. Rides the **call output path** (not the notification stream) at the same 4-stop pad, so the ringback and the conversation that follows land at one loudness. Loops on chirp's published `RING_REPEAT_GAP_SECS` until any Outgoing teardown edge drops `ActiveCall::ringback`. Honors the `notify.ring_call` tick.

Same on desktop and Android (it's the shared platform audio layer); silent on Redox.

**It is also the calibration probe.** A ringback is a known signal played into the room while the near human is provably not talking — the far-talks-alone condition the in-call learner otherwise waits a whole conversation for. The session is open, so the mic captures while it plays; the same `learn::Learner` runs on the ring and publishes a probe. On answer, the engine seeds from that freshly measured `(g, delay)` instead of a stored profile from another day. Field waves 1-2 failed exactly there: the learner never armed inside a 41 s call, so both sides ducked on stale seeds.

The probe volume-normalizes with the live `current_volume_db()` exactly as the engine's learner does — it publishes `g` at unit volume because the predictive duck re-scales by `vol_lin` at use. On Android the mirrored stream is `STREAM_MUSIC`, which is genuinely the knob governing our `USAGE_MEDIA` render (mirroring `STREAM_VOICE_CALL` was the earlier bug: it normalized by a slider that did nothing to our audio).

Session ownership: ringback opens the audio session; on stop it closes it **unless** the engine took over (`media_sink_live()`). The engine's own `audio::start()` is a no-op against a live session, so ringback → call never tears the device down and never clicks.

## 3. Ring — someone is waving us

`chirp::Chirp::ring_from_hash`: the contact's ding chord **held flat** (no decay envelope, no saw gate, no hammer, no room) under a **sin³ arc, phase 0 → 9π** across each 0.5 s burst — ten zeros counting the ends, nine lobes, four inverted. Two bursts, 0.25 s apart, = one "ring-ring"; the 2 s repeat gap is the caller's, deliberately not rendered into the clip.

| | desktop | Android foreground | Android background/locked |
|---|---|---|---|
| surface | OS notification (`notify-send` / `osascript`) | full-screen ring panel | `CATEGORY_CALL` notification + `fullScreenIntent` + Answer/Decline actions |
| sound | rodio thread, loops cadence + polled 1.2 s gap | `playRingAlert` → looping `AudioTrack` | `postCallNotification` → same looping track |
| path | default output device | `USAGE_NOTIFICATION_RINGTONE` | same |
| haptic | — | repeating waveform (`USAGE_COMMUNICATION_REQUEST`) | same |
| stop | `RingGuard` drop on any teardown edge | `stopRingLoop` via `cancelCallNotification` | same |

Both platforms now **loop until a stop edge, no timers**. Android looped nowhere before 2026-09-03 — `playChirp` fired once and released, despite a doc comment claiming otherwise; it rang once per offer while desktop looped. The loop lives in the HAL (`MODE_STATIC` + `setLoopPoints(-1)`) so there's no timer and no wakeup; the repeat gap is appended Kotlin-side from a gap-ms int rather than marshalling 2 s of zeros per ring.

`notify.ring_call` unchecked = the notification still posts (a call is never invisible), only the audio and haptic are withheld. That tick was read **only** inside the desktop cfg block until 2026-09-03, so Android ignored it — survivable when the ring was one shot, not when it loops forever.

The ring deliberately **bypasses the `will_ding` gates**: a call is the one always-ring event (2026-08-18).

## 4. Message ding

`chirp::Chirp::from_hash` — the same instrument, struck once, with its room: the identity voice the ring is conjugated from.

- **Desktop**: detached thread, `play_blocking` on the default device. Gated by `will_ding` (not looking at that conversation, no live sibling clearer, real friend row — never a chain-weave probe or a fleet-sync frame) and deduped on `msg_hp`. The visual toast has its own gate (nothing while attended).
- **Android**: `postMessageNotification` → one-shot `playChirp` on `USAGE_NOTIFICATION` + one-shot haptic. Survives Doze — FCM wakes the app, Rust renders the WAV + haptic envelope, Kotlin plays them, so the OS default tone never fires and the sound is per-contact even from deep sleep. The pubkey never crosses JNI; only rendered audio does.
- **Redox**: none.

## 5. V-chirp connect probe — [`call/vchirp.rs`](../src/call/vchirp.rs)

Every call opens with the SAME one-second sound on both ends (2026-09-07; lengthened 250ms → 1s 2026-09-08 — only the sweep's middle is audible thru a phone transducer): a 200Hz→20kHz log sweep and its exact time-reversal overlaid, played **unpadded** thru `queue_playback` on the live route while BOTH directions hold — no mic TX, RX decoded but not rendered — so the chirp is the only thing in the room. When the capture window (~1.6s, or a 4s deadline if the mic never grants) closes, audio connects immediately; the fit runs off-thread and seeds the duck a beat later.

The fit: matched-filter each sweep leg separately (sample-precise delay, polarity-blind), then take g as the median per-bin envelope ratio at the matched delay — the learner/duck's unit, total leak not just direct path. The V shape is load-bearing three ways: the legs' lag **mean** is the true delay with clock skew cancelled, their **difference** measures render-vs-capture clock skew directly (the thing splice counters only see indirectly), and leg agreement is the corruption gate (speech/movement → legs disagree → rejected, never stored). ~43dB of processing gain means a simultaneous "hello" barely dents it. No fade shaping — the sweep starts at a zero crossing on the 200Hz end and the device's own taper rounds the rest, which also leaves the raw sweep usable as a loop frequency-response probe later.

Results persist thru the learned-profile drain (solid tier) and seed the engine's predictive duck live, outranking any stored/ringback seed — a measurement of THIS route THIS second. A no-coupling verdict (headset-class route) is legal: the duck stays reactive, the floor is still taken.

This probe replaced the Wave calibration ritual AND its doctrine: the Settings→Wave page is gone, and the three gates (place-call, answer, mid-call mic-mute on uncalibrated routes) are retired — "uncalibrated" is no longer a state a call can be in, because the probe runs before any voice does. Voice profiles (mic gain, floor) still accumulate from the in-call learner's evidence.

## 6. Recording preview

Ended-screen playback of the live spool through the same mono downmix and `queue_playback`, **unpadded**. Desktop and Android; one owner of the audio session at a time (starting a preview stops the prior one).

---

## Who stops what

Every stop is an **edge, never a timer** — the project rule.

| sound | stopper |
|---|---|
| ring (desktop) | `RingGuard` drop — decline, sibling answer, caller hangup, call overwrite |
| ring (Android) | `stopRingLoop` from `cancelCallNotification`, called by `stop_ring_alert_platform` |
| ringback | `RingbackGuard` drop when `ActiveCall::ringback` clears (answered, declined, hung up, glare-folded) |
| wave audio | engine `stop()` → `clear_media_sink` + `audio::stop()` |
| preview | replaced by the next preview, or call teardown |

## Known gaps

1. **Android route label lies.** Sniffer reports `"earpiece"` for a loudspeaker path; calibration profiles are keyed on that string, so they'd collide if a real receiver path ever lands.
2. **No receiver/speaker choice on Android.** One path, one loudness, set by the 4-stop pad. Restoring a real earpiece mode means taking the 80 ms buffer floor back in that mode.
3. **Ring cadence length is fixed** at 0.5 s bursts / 0.25 s inner gap / 2 s repeat. Not user-tunable, not per-contact beyond the digest.
4. **Redox is silent** end to end — signaling only.
5. **The volume rocker adjusts media, not call volume** on Android — the direct consequence of the `USAGE_MEDIA` trade. Self-consistent (the mirror reads the same stream the render uses), and the ring/ringback split still matches the customary model: inbound ring on the ringtone slider, ringback on the call/media one.
6. **Desktop ring gap is polled** (24 × 50 ms) rather than event-driven; the poll exists only so a stop edge lands within ~50 ms.
