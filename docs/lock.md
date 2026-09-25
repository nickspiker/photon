<!-- Nick's LOCK specification, pasted 2026-09-24, preserved verbatim. Decisions taken since are recorded in docs/waves.md ("The canonical wave") and in the status block at the end of this file. -->

# Photon Wave: LOCK Specification

Universal-time alignment for Photon audio (Wave), with the video (Beam)
outline that follows from it. Written for implementation by a coding agent.
Rust, cross-platform (Android, macOS/iOS via FFI, Linux).

Style: no floats in names or on the wire. Integers name things, floats are
allowed only inside estimators.

---

## 0. Goals and non-goals

Goals
- Every audio sample Photon captures has a globally unique integer name.
- Two devices anywhere on Earth agree, to within stated uncertainty, on the
  true instant any named sample was captured.
- Playout is scheduled against true time, not against arrival time.
- Every stamp carries its own uncertainty and lock source.
- Drift is corrected at the source. Receivers correct only their own DAC.

Non-goals (this phase)
- Multi-party mixing.
- Phase-coherent multi-mic (beamforming). Design must not preclude it.
- Video. Outlined in section 10 only.
- Browser clients.

---

## 1. Definitions

### 1.1 Eagle Time (measurement timescale)

- `OPS = 1_420_407_826` oscillations per second. One Eagle second is one
  SI second on the geoid. The constant is the barycentric hydrogen line as
  received on Earth. Do not change it.
- Epoch: `1969-07-20T20:17:48 TAI` exactly (integer TAI second, by fiat).
  The literal touchdown instant was about 20:17:47.575 TAI; the epoch is
  pinned 425 ms later so Eagle second boundaries coincide with TAI, GPS
  and UTC second boundaries.
- `EagleCount = i64` oscillations since epoch. Range ±206 years.
- Continuous. No leap seconds. UTC is a display conversion only.

### 1.2 Grid (naming)

- `RATE = 48_000` samples per second, fixed for all devices.
- `SampleIndex = i64` samples since the Eagle epoch. Sample k is nominally
  at `k / 48000` seconds after epoch, exact in rationals.
- Packet = 960 samples (20 ms). Packet boundaries are at multiples of 960
  from the top of each Eagle second. 50 packets per second.
- Grid names are exact. Eagle stamps are measurements of where a named
  sample actually was. The difference is phase error.

### 1.3 TrueClock

A per-process model mapping the OS monotonic clock to Eagle Time:

```
eagle(mono) = offset + mono * (1 + rate_ppm * 1e-6)
```

with `uncertainty_ns` and `lock_source`. Never modifies the system clock.

### 1.4 Lock source

```rust
enum LockSource { Gnss, Ptp, Ntp, Peer, Holdover, Free }
```

`Peer` = disciplined only against another Photon device's clock (relative
lock, no absolute truth). `Free` = no discipline; stamps are meaningless
as absolute time and must be marked.

---

## 2. Required changes to `vsf::types::eagle_time`

1. Epoch constant expressed in TAI, not chrono `Utc`. Add
   `EAGLE_EPOCH_TAI_SECS` (seconds since 1970-01-01 TAI, negative).
2. Remove all `f64` paths in absolute conversions. Implement in `i128`:
   - `from_tai_ns(secs: i64, nanos: u32) -> EagleCount`
   - `to_tai_ns(EagleCount) -> (i64, u32)`
   Use `div_euclid` / `rem_euclid`, round half up.
3. `eagle_time_now()` must take TAI from a `TaiSource` trait, not
   `Utc::now()`. Provide implementations: `GpsTai` (GPS + 19 s), `PtpTai`,
   `NtpTai` (UTC + leap table). Keep chrono only for display.
4. Ship a leap-second table (TAI − UTC, currently 37 s; last leap 2016-12-31)
   as data, updatable without a code change.
5. Fix doc comment: the epoch is touchdown, not the transmission.
6. Test vectors (must pass):
   - epoch → count 0
   - `from_tai_ns` / `to_tai_ns` round trip exact for 10^6 random instants
   - 1 second → exactly `OPS`
   - Second boundary property: `count % OPS == 0` iff TAI nanos == 0

---

## 3. Grid conversions (new module `grid`)

```rust
pub const OPS: i128 = 1_420_407_826;
pub const RATE: i128 = 48_000;
pub const PACKET: i64 = 960;

pub fn sample_to_eagle(k: i64) -> i64 {
    let n = k as i128 * OPS;
    (n.div_euclid(RATE) + ((n.rem_euclid(RATE) * 2 >= RATE) as i128)) as i64
}

pub fn eagle_to_sample(e: i64) -> i64 {
    let n = e as i128 * RATE;
    (n.div_euclid(OPS) + ((n.rem_euclid(OPS) * 2 >= OPS) as i128)) as i64
}

/// Signed phase error in samples: measured instant minus nominal slot.
pub fn phase_error_samples(k: i64, measured: i64) -> f64 {
    (measured - sample_to_eagle(k)) as f64 * RATE as f64 / OPS as f64
}

pub fn packet_start(k: i64) -> i64 { k.div_euclid(PACKET) * PACKET }
```

Invariants (tests):
- `eagle_to_sample(sample_to_eagle(k)) == k` for all k (exact).
- `|sample_to_eagle(k) - k*OPS/RATE| <= 0.5` oscillation.
- `packet_start` of a top-of-second sample equals that sample.

---

## 4. TrueClock: discipline

### 4.1 Inputs

- Round-trip exchanges from the existing time crate (`nunc-time`), against
  a reference that reports TAI (or Eagle). Each exchange yields
  `(t1, t2, t3, t4)`: local send, remote receive, remote send, local
  receive. Remote times in Eagle; local times in monotonic ns.
- Optional GNSS time from the platform (Android `GnssClock`; Apple has no
  raw GNSS time API, use NTP/PTP there).

### 4.2 Estimator

Per exchange:
```
delay  = (t4 - t1) - (t3 - t2)          // round trip minus remote hold
offset = ((t2 - t1) + (t3 - t4)) / 2    // assumes symmetric path
```
- Keep a sliding window (e.g., 64 samples over 5 minutes).
- Use only the lowest-delay quartile of the window for the fit (minimum
  filter; asymmetric queuing shows up as high delay).
- Linear regression of `offset` against `mono` over the filtered set gives
  `offset` (intercept) and `rate_ppm` (slope).
- `uncertainty_ns = max(min_delay/2, 3 * residual_stddev) + holdover_age
  * rate_uncertainty`.

### 4.3 Policy

- Never step the mapping while a call is active. Slew: limit the change of
  `offset` to 50 µs per second. Rate changes are unlimited.
- Loss of reference: keep last `offset` and `rate_ppm`, set
  `LockSource::Holdover`, grow `uncertainty_ns` at the last measured rate
  uncertainty (default 2 ppm if unknown → 2 µs/s).
- Uncertainty above 5 ms: mark stamps `degraded`; grid alignment
  guarantees do not apply; the call continues.
- Log every fit: window size, min delay, residual stddev, rate_ppm.

### 4.4 API

```rust
pub struct TrueClock { /* Arc<RwLock<Model>> */ }
impl TrueClock {
    pub fn now(&self) -> Stamp;                       // Eagle now + uncertainty
    pub fn eagle_of(&self, mono_ns: i64) -> Stamp;    // convert a past instant
    pub fn mono_of(&self, eagle: i64) -> i64;         // inverse, for scheduling
    pub fn feed(&self, t1: i64, t2: i64, t3: i64, t4: i64);
    pub fn lock(&self) -> (LockSource, u64 /*uncertainty_ns*/);
}
pub struct Stamp { pub eagle: i64, pub uncertainty_ns: u32, pub source: LockSource }
```

---

## 5. Capture path (sender)

### 5.1 Hardware timestamps

Every platform must supply `(frame_position: u64, mono_ns: i64)` pairs
for the input stream. Sources:

| Platform | API | Notes |
|---|---|---|
| Android | `AAudioStream_getTimestamp(CLOCK_MONOTONIC)` or `AudioRecord.getTimestamp` | quality varies by device; log residuals per model |
| iOS/macOS | `AudioTimeStamp.mHostTime` + `mSampleTime` (Core Audio) | convert mach ticks to ns via `mach_timebase_info` |
| Linux | ALSA `snd_pcm_status` hardware timestamps, or PipeWire graph clock | |

If the audio backend in use (e.g., cpal) does not expose these, bypass it
for timestamps; do not fall back to "time when callback ran".

**Acoustic reference point.** A sample's stamp means the instant sound hit
the capsule, not when the buffer arrived. Sigma-delta ADCs have a fixed
decimation-filter group delay (typically 0.2 to 1 ms). Subtract the
codec's datasheet group delay from every capture stamp. Store the value
used as `adc_delay_ns` in device config; log it. The playout side adds the
DAC reconstruction-filter delay the same way (section 7.2).

### 5.2 ADC rate regression

- Collect `(frame_position, eagle(mono_ns))` pairs each callback.
- Sliding window 10 s. Fit `eagle = a + b * frame_position`.
- `b` is Eagle counts per sample; nominal is `OPS/RATE = 29591.83`.
  `adc_ppm = (b / 29591.83 - 1) * 1e6`. Expect ±20 to ±100 ppm.
- Residual stddev is the timestamp quality metric. Log it.

### 5.3 Naming captured samples

For each captured buffer, first hardware frame `f0`:
```
eagle_f0 = a + b * f0                       // measured capture instant
k_target = eagle_to_sample(eagle_f0)        // grid slot it should occupy
phase    = phase_error_samples(k_target, eagle_f0)   // in [-0.5, 0.5] initially
```
The corrector's job: emit the captured stream so that the sample which was
physically captured nearest grid slot k is emitted as sample k. Drift
(adc_ppm) makes `phase` walk; the corrector holds it near zero.

### 5.4 Corrector

Two implementations behind one trait; the loop is shared.

```rust
trait Corrector { fn process(&mut self, input: &[i16], cmd: Command, out: &mut Vec<i16>); }
enum Command { None, Insert, Delete, Ratio(f64) }
```

A. `SlipCorrector` (phase 1, voice)
- Insert = duplicate one sample; Delete = remove one sample.
- Placement: within the buffer, pick index n minimizing
  `|x[n] - x[n-1]| + 0.25 * |x[n]|` (flat spot preferred, near-zero second).
  Never slip inside the first or last 8 samples of a buffer.
- Max one slip per 10 ms. If the loop demands more, switch to B.

B. `AsrcCorrector` (phase 2, music, multi-mic)
- Polyphase or windowed-sinc fractional resampler with ratio `1 + r`.
- `r` set by the loop; changes limited to 1 ppm per 10 ms to avoid
  audible pitch steps.

Loop (runs every 100 ms):
```
err       = phase (samples), low-pass filtered (tau 1 s)
integ    += err * ki
out       = err * kp + integ
```
- Defaults: `kp = 0.05`, `ki = 0.002`, deadband 0.5 sample.
- `|out| > deadband` → one Insert/Delete (A) or `r = -out/RATE` per
  100 ms (B).
- Jitter in the *timestamps* must not reach the loop: the regression
  already filters it. Never feed raw callback timing into `err`.

### 5.5 44.1 kHz devices

Resample to 48 kHz in the same ASRC stage (`AsrcCorrector` with base ratio
48000/44100 times the correction). Never a separate pass.

### 5.6 Encoding

- Opus, 20 ms frames, 48 kHz input. Enable in-band FEC (LBRR) for WAN.
- Encoder input frame N covers grid samples `[N*960, N*960+960)` exactly.
  This is the invariant that makes names meaningful; assert it.

---

## 6. Wire format (VSF)

One packet:

| Field | Type | Notes |
|---|---|---|
| `k0` | i64 (VSF exponential-width; delta vs previous packet allowed) | grid index of first sample |
| `stamp` | Eagle e6 | measured capture instant of sample k0 |
| `unc_ns` | u32 | uncertainty of `stamp` |
| `src` | u8 | LockSource |
| `adc_ppm` | i16 | informational; receivers may use for diagnostics |
| `payload` | bytes | Opus frame |
| `sig` | bytes | Photon identity signature over all of the above plus `prev_hash` |
| `prev_hash` | 16 bytes | truncated hash of previous packet (chain) |

- `k0 % 960 == 0` always. Receiver rejects packets that violate this.
- Absolute `k0` at least once per second; deltas otherwise.
- Replay guard: reject if `stamp` implies arrival earlier than possible
  (arrival_eagle < stamp − unc) or later than `max_age` (default 2 s).

---

## 7. Playout path (receiver)

### 7.1 Buffer

- Keyed by `k0`. Insert on arrival. Missing packets are known by index gaps.
- Target latency `L` (in samples): initial 60 ms WAN / 20 ms LAN.
  Recompute from the 95th percentile of `(arrival_eagle − stamp)` over
  30 s. Apply changes to `L` only during silence (Opus DTX or energy below
  threshold), as a single jump, never via slips.

### 7.2 Scheduling

Each output callback asks for samples starting at DAC frame position `p`.
- Output regression (same as 5.2, on the output stream) gives
  `eagle_of_dac(p)`.
- `k_play = eagle_to_sample(eagle_of_dac(p) + dac_delay − L_eagle)`, where
  `dac_delay` is the DAC's reconstruction-filter group delay, so `k_play`
  is the sample that should be leaving the speaker at that instant.
- Fill from the buffer at `k_play`. Gap → Opus PLC (or FEC-recovered
  frame). Never stretch to cover loss.

### 7.3 DAC drift loop

The DAC crystal drifts against Eagle exactly like the ADC. Run the same
loop as 5.4 on the output side, with `err = phase error between k_play
and the sample actually at the DAC`. Slip or ASRC on the playout stream.

### 7.4 AEC reference

Tap the echo-cancellation far-end reference **after** the playout
corrector, i.e., the exact samples handed to the DAC, together with the
DAC-side Eagle stamp. The near-end (mic) stream carries its own stamps.
The AEC aligns by Eagle time, not by buffer position.

### 7.5 Peers without lock

If a peer's `src` is `Free`, or `unc_ns` > 5 ms:
- Fall back to relative timing: treat the peer's `stamp` as an arbitrary
  clock, discipline a per-peer offset from arrival statistics, and mark
  the call `unlocked`.
- The UI shows lock state per direction.

---

## 8. Telemetry

Emit per call, per direction, once per second:
- `one_way_latency_ms` = presented/played Eagle − capture `stamp`
- `lock_source`, `uncertainty_ns` (local and remote)
- `adc_ppm`, `dac_ppm`, regression residual stddev (both sides)
- slip count / ASRC ratio, buffer level (samples), loss count, PLC count
- `L` and any change events

Report one-way latency back to the peer in the signaling channel. It is
data for tuning and display. It must **never** feed the TrueClock.

---

## 9. Verification and acceptance

### 9.1 Clap test (the credibility test)

Two devices, same room, 1 m apart, both locked. One clap.
- Find the transient's grid index on each device (first sample above
  threshold after a quiet window).
- `error = |k_a − k_b| / 48 ms − acoustic_offset` where acoustic offset
  is `distance_a − distance_b` over 343 m/s.
- Acceptance: error ≤ 1 ms (48 samples) on LAN with NTP-class lock;
  ≤ 5 ms on WAN. Record per device model.

### 9.2 Loopback drift test

Play a 1 kHz tone on device A, record on B for 10 minutes. The recorded
tone's frequency, measured against B's grid, must be 1000.000 Hz ± 0.02 Hz
(20 ppm) with correction on, and show the raw crystal error with it off.

### 9.3 Unit tests

- Sections 2 and 3 invariants.
- Loop stability: simulated ADC at +80 ppm reaches |phase| < 0.5 sample
  within 30 s and stays there; slip rate ≈ 3.8/s (A) or r ≈ −80 ppm (B).
- Slip audibility: THD+N on a 200 Hz sine with slipping at 4/s stays
  below −40 dB (A). Document the result for 2 kHz as well (expected worse;
  this is the trigger for B).
- Packet invariants: `k0 % 960`, replay window, chain hash.

---

## 10. Video (Beam) outline

Same architecture, one level up. Do not implement until 9.1 passes.

1. **Names.** `FrameIndex = i64`, rate as rational `(num, den)`; live
   capture integer-only (24, 25, 30, 50, 60, 120). Frame n nominal at
   `n * den / num` seconds after epoch. 60 fps = 800 audio samples.
2. **Frame stamp = center.** The frame's Eagle stamp is the mid-exposure
   instant of the middle row, the temporal and vertical centroid of what
   the frame recorded:
   `center = start_first_row + exposure/2 + readout/2`.
   Carry the geometry alongside so any row's instant is recoverable:
   `exposure_duration`, `readout_duration` (0 = global shutter). The stamp
   is the name; the geometry is the proof. On Android the inputs are
   `SENSOR_TIMESTAMP` (verify per device whether it marks exposure start or
   readout start), `SENSOR_EXPOSURE_TIME`, and `SENSOR_ROLLING_SHUTTER_SKEW`.
   Because a rolling-shutter row's exposure ends at its readout, changing
   exposure moves the center by half the change; soft genlock (item 3)
   steers the *center* onto the grid, nudging readout timing when exposure
   changes.
3. **Soft genlock.** Android: nudge `SENSOR_FRAME_DURATION` to slew the
   sensor's phase onto the grid, measure via `SENSOR_TIMESTAMP` (check
   whether it marks start-of-exposure or start-of-readout per device).
   Apple: `AVCaptureDevice.activeVideoMinFrameDuration` for the same
   effect; true genlock only on hardware that supports
   `AVExternalSyncDevice`.
4. **Correction.** Whole frames only. Repeat/drop when |phase| > 0.5 frame,
   with hysteresis (act at 0.6, release at 0.4).
5. **Lip sync.** Audio is master. Target 0 offset. If forced, delay audio;
   never advance it. Alarm at audio early > 15 ms or late > 45 ms.
6. **Presentation stamps.** `present_start` + `scanout_duration` from the
   platform present callback (Android present fences; Apple
   `MTLDrawable.presentedTime`). One-way glass-to-glass = present −
   exposure_start, reported like 8.
7. **Wire.** Same header as section 6 with `frame_index`, `rate`,
   geometry fields; payload = encoded frame. Absolute stamps on keyframes.
8. **Legacy.** Rational rates (incl. 1000/1001) accepted on import only.
   Export to SMPTE 12M/drop-frame at the boundary. Never internal.

---

## 11. Implementation order

1. `vsf` Eagle fixes + `grid` module + tests (section 2, 3).
2. `TrueClock` with `nunc-time` feed, logging, holdover (section 4).
3. Capture timestamps on one platform (pick the one with the best
   hardware timestamps; likely macOS), regression, logging only.
4. `SlipCorrector` + loop, sender side. Verify with 9.2.
5. Wire format and receiver scheduling (6, 7.1, 7.2).
6. DAC loop (7.3), AEC tap (7.4).
7. Clap test (9.1). Ship when it passes on two device models.
8. Second platform. Then `AsrcCorrector`.

---

## 12. Open questions (decide before step 5)

- Signature scope: per packet, or per second with a Merkle root? Per
  packet is simplest; cost is ~64 bytes per 20 ms.
- Bluetooth output: latency is large and unobservable. Proposal: detect,
  set `LockSource::Free` on the playout side, keep capture locked.
- `L` policy on asymmetric routes: symmetric `L` on both ends, or each
  end minimizes its own? Symmetric keeps conversational turn-taking fair.

---

## Status and decisions since (maintained by the implementer)

- §2–3 (vsf Eagle integer conversions, `grid` module): BUILT 2026-09-24 (vsf `eagle_time.rs`, `grid.rs`; photon pin test `nunc_and_vsf_agree_to_the_oscillation`).
- §4 TrueClock: BUILT 2026-09-25 (photon `network/true_clock.rs` estimator, `network/time_base.rs` the process clock). Fed by every nunc consensus's precise exchanges (NTP/NTS/Roughtime, outliers dropped; HTTPS-only draws fall back to the consensus as one wide exchange). Deviations from the spec text, on purpose: the minimum filter runs per 5-minute BIN (the reference arrives in bursts an hour apart, so a whole-window quartile could keep one burst and never see a rate), the window holds 6 h, holdover begins 2 h after the newest exchange, and a wave starting on a consensus older than 10 min asks for a fresh one. The FGTW window refusal still steps (resets the window). nunc fixed on the way: NTP sources read rtt/2 ahead, NTS kept whole seconds.
- 2026-09-25 (Nick): one spool per identity (distinct vault objects), frames named on the absolute 5 ms grid (`k0 % 240 == 0` — the engine frame, a quarter of §1.2's 20 ms packet), the first frame padded with zeros rather than waiting for a boundary, the far channel stored under the SENDER's frame names so all channels align at microphone time. See docs/waves.md "One spool per identity".
- §5.2–5.4 (the sender's aligner): BUILT as a pure module, `wave/align.rs`, NOT yet wired to a capture path. Measured on the §9.3 tests: +80 ppm held to 0.093 samples of filtered phase after 30 s at 3.84 deletions/s; ±250 µs HAL jitter absorbed; slip THD+N −44.8 dB at 200 Hz (spec < −40), −24.8 dB at 2 kHz (the documented trigger for the ASRC). The spec's loop is read as a RATE loop — the PI output is samples of correction per 100 ms tick, realized as whole slips by accumulation — which is what makes its gains settle inside its own 30 s acceptance; a literal bang-bang on the output diverged.
- W1 (2026-09-25, the engine): the aligner sits in the wave engine's TX path; every datagram names its source window by its first frame's grid number (u32 = k0 / 240 mod 2^32) and the repaired window's likewise, fills carry it too, and BOTH channels' spool records are stamped `sample_to_eagle(k0)` — our mic by our names, the far party's by the names its sender put on the wire — so the keep slots every party at microphone time. Capture stamps come from the HAL on CLOCK_BOOTTIME (Android) or the callback less cpal's capture delay (desktop), mapped thru a lock-free TrueClock snapshot. A WIRE FLAG DAY. Not yet built: one vault object per identity at keep (the PHWAVE9 container still interleaves), the receiver's playout scheduling against true time (§7), the DAC loop (§7.3), the clap test (§9.1).
- §12 signature scope: DECIDED — one signature per party per wave at the truing-up, over its channel's canonical frame-set root plus the sender's manifest; not per packet (64 B × 50/s = 25.6 kbps exceeds the 16 kbps sublight rung). See docs/waves.md "The canonical wave". §6's per-packet `sig` / `prev_hash` fields are superseded accordingly.
- Reconstruction: the wave is the FRAME SET keyed by grid index, sorted by grid index then capturing party's handle proof; decode in contiguous spans, a hole terminates the run; no mix at rest; rows are spans. See docs/waves.md.
- At-rest pages (2026-09-24 brainstorm): stored rungs 16/32/64/128 kbps, 4,000 B of audio per 4,096 B page aligned to the absolute Eagle grid; sublight pages start on EVEN Eagle seconds; waves begin on a 1 s boundary.
