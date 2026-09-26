<!-- Nick's Tukutahi draft v0.1 (2026-09-25), preserved verbatim as he sent it — food for thought, not yet discussed or adopted. -->

# Tukutahi

Timing, rate declaration, and alignment for video and audio.
Companion: the Manawa marking grid (section 5).

Version 0.1 draft, 2026-09-25. Author: Nick Spiker. Contributor-not-owner:
anyone may implement, extend, or fork. Nothing here is owned.

Tukutahi (te reo Māori: simultaneous, in sync) defines how footage declares
its rate, how frames and samples are named, and how those names are tied to
true time. Manawa (heartbeat) is the cadence at which measurements are
written down.

Assumes Eagle Time as the timescale. Assumes nothing about the platform;
conformance levels (section 9) describe what a platform can guarantee.

Conventions: MUST / SHOULD / MAY as in RFC 2119. No floats anywhere in a
declaration, a name, or on the wire. Floats are permitted only inside
estimators.

---

## 0. Scope

In scope
- Declaring the rate and phase of a video or audio stream.
- Naming every frame and sample with a globally unique integer.
- Recording, sparsely, when named frames and samples actually occurred, with
  uncertainty and provenance.
- Recording capture and presentation geometry so any row's instant is
  derivable.
- Interop with legacy rates and timecode at the boundary.

Out of scope
- Codecs, containers beyond the VSF encoding in section 13, transport.
- Clock discipline algorithms (see the Wave LOCK spec).
- Colour (see VSF RGB).

---

## 1. Terms

| Term | Meaning |
|---|---|
| Eagle Time | The timescale. Integer oscillation count since the Eagle epoch. Section 2. |
| Stream | One video or one audio essence with a single declaration. |
| Declaration | The header that fixes a stream's rate, phase, cadence, and level. Section 3. |
| Name | The integer identity of a frame or sample: `(stream, index)`. Section 4. |
| Nominal time | Where a name should be: `epoch + index × period + phase`. |
| Stamp | A measured Eagle instant, with uncertainty and lock source. |
| Manawa | The marking cadence: how often stamps are written. Section 5. |
| Mark | One record written on the Manawa cadence. |
| Lock | Which reference a device's clock is disciplined to, and how well. Section 8. |
| Level | What a device can guarantee about its stamps. Section 9. |

---

## 2. Timescale: Eagle Time

Normative constants:

```
OPS   = 1_420_407_826          oscillations per second (SI second on the geoid)
EPOCH = 1969-07-20T20:17:48 TAI (integer TAI second, by fiat)
```

- `EagleCount` is a signed 64-bit integer of oscillations since `EPOCH`.
- Eagle Time is continuous. Leap seconds do not exist in it. UTC is a
  display conversion and MUST be done with a leap-second table.
- Eagle second boundaries coincide with TAI, GPS, and (since 1972) UTC
  second boundaries. This is the reason `EPOCH` is an integer TAI second.
- `OPS` is not divisible by any frame or sample rate. That is fine: Eagle
  Time is for measurements. Names are rational (section 4).

Conversion (normative, integer only):

```
to_eagle(secs_tai, nanos) = (secs_tai - EPOCH_SECS) * OPS + nanos * OPS / 1e9
                            (i128, round half up)
```

---

## 3. Declaration

Written once per stream, before any frame or sample. Immutable for the
life of the stream. A change of any field is a new stream.

| Field | Type | Meaning |
|---|---|---|
| `kind` | enum | `video` or `audio` |
| `rate_num`, `rate_den` | u32, u32 | Frames (or samples) per second as a rational. `gcd = 1`. |
| `phase_num`, `phase_den` | u32, u32 | Offset of index 0 from the epoch, as a fraction of one period. `0/1` unless declared. See section 11. |
| `manawa_period` | u32 | Marking cadence in whole periods. Default: one second's worth (`rate_num / rate_den` when integer). |
| `level` | u8 | Conformance level (section 9). |
| `source` | bytes | Photon identity of the capturing device. |
| `sensor_rows` | u32 | Video only. Rows read out per frame. `1` for global shutter or unknown. |
| `native` | bool | `true` if captured under this declaration; `false` if imported. |

Rate constraints:
- `native = true` MUST have `rate_den = 1` and `rate_num` in
  `{24, 25, 30, 48, 50, 60, 96, 100, 120, 240}` for video, `48000` for
  audio.
- `native = false` MAY use any rational, including `24000/1001`,
  `30000/1001`, `60000/1001`, and `44100/1`.

Rationale: 25 and 50 stay because mains flicker is still real under
cheap LED lighting. Nothing ×1000/1001 is ever native. See section 12.

---

## 4. Naming

A frame or sample's name is its stream plus a signed 64-bit `index`.

Nominal instant of index `n`, in seconds after the epoch, exact in rationals:

```
t(n) = (n + phase_num / phase_den) * rate_den / rate_num
```

Nominal Eagle count (rounded once, never stepped):

```
eagle(n) = round( ((n * phase_den + phase_num) * rate_den * OPS)
                  / (rate_num * phase_den) )        // i128, round half up
```

Nearest name for an Eagle instant `e`:

```
index(e) = round( e * rate_num * phase_den / (rate_den * OPS) ) * ... 
```
Implementations MUST use the reference code in Appendix B, which handles
the phase term correctly. Round trip `index(eagle(n)) == n` MUST hold for
all `n`.

Properties:
- Names never wrap. i64 at 240 fps covers 1.2 million years.
- For integer rates with zero phase, index `n` lies on a second boundary
  iff `n mod rate_num == 0`.
- Rounding error of `eagle(n)` is at most half an oscillation (352 ps).
  Nothing in this spec can measure that.

Audio and video on the same epoch align exactly: at 48 kHz and 60 fps,
frame `n` spans samples `[800n, 800n + 800)`. At 24 fps, 2000 samples.

---

## 5. Manawa: the marking grid

A stream does not stamp every frame. It writes a mark every
`manawa_period` periods. Names are implicit between marks (section 6).

### 5.1 Placement

- Video, integer rate, zero phase: marks fall on indices divisible by
  `manawa_period`. With the default cadence, that is the first frame of
  each Eagle second.
- Audio: marks fall on indices divisible by `manawa_period`. Default
  `48000`, i.e. the first sample of each second.
- Legacy rational rates: no frame lands on a second boundary. Mark the
  frame whose nominal time is nearest each second boundary. Its name is
  still exact; its stamp shows the offset.

### 5.2 Mark record

| Field | Type | Meaning |
|---|---|---|
| `index` | i64 | The named frame or sample this mark describes. |
| `stamp` | EagleCount | Measured instant of that frame's canonical point (section 7). |
| `unc_ns` | u32 | 1-sigma uncertainty of `stamp`, in nanoseconds. |
| `lock` | enum | Section 8. |
| `rate_ppm` | i32 | Measured local clock rate error, informational. |
| `root` | 32 bytes | Merkle root of the per-frame hashes since the previous mark. |
| `prev` | 32 bytes | Hash of the previous mark. |
| `sig` | bytes | Signature by `source` over all fields above. |

### 5.3 Meaning of a mark

Between two marks, the instant of frame `n` is interpolated linearly
between the stamps, and its uncertainty is the larger of the two `unc_ns`
plus `|rate_ppm| × distance` in time. A reader MUST NOT extrapolate past
the last mark by more than one `manawa_period` without raising the
uncertainty accordingly.

Level 2 streams (section 9) MAY declare interpolation exact.

### 5.4 Cadence

`manawa_period` is a stream parameter. Default one second. A live stream
wanting fast relock MAY use 100 ms. A file MAY use 10 s. Readers MUST
accept any value ≥ 1.

---

## 6. Per-frame and per-sample records

Between marks, each frame carries:

| Field | Type | Meaning |
|---|---|---|
| `delta` | i64, VSF exponential width | `index − previous index`. Implicit `1`; written only when not 1. |
| `hash` | 32 bytes | Hash of the frame payload. Feeds the next mark's `root`. |
| `geometry` | optional | Written only when geometry changes (section 7). |

A `delta ≠ 1` is a declared gap. Dropped frames MUST be declared, never
silently skipped. A reader encountering an undeclared discontinuity
(hash chain break, or a mark whose `index` disagrees with the running
count) MUST flag the stream as broken from that point.

Audio samples carry nothing individually. Packets carry the index of
their first sample, exactly as a frame carries `delta`.

---

## 7. Geometry

### 7.1 Canonical point

A frame's stamp refers to its **temporal and vertical center**: the
midpoint of exposure of the middle row. Not exposure start. Not readout
start. This is the instant the frame is most nearly "of".

### 7.2 Capture geometry

Declared at stream start and re-declared on change:

| Field | Type | Meaning |
|---|---|---|
| `exposure` | EagleCount | Exposure duration of one row. |
| `readout` | EagleCount | Time from first row start to last row start. `0` = global shutter. |

From these and `sensor_rows` (section 3), the mid-exposure instant of row
`r` in frame `n`, with `R = sensor_rows`:

```
row_center(n, r) = stamp(n) + readout * (r - (R - 1)/2) / (R - 1)
exposure_start(n, r) = row_center(n, r) - exposure / 2
```

Any consumer can therefore reconstruct any row's instant, so rolling
shutter is a parameter, not a special case.

### 7.3 Presentation geometry

A displaying device writes its own marks on the same cadence:

| Field | Type | Meaning |
|---|---|---|
| `index` | i64 | Frame presented. |
| `present` | EagleCount | Instant of mid-scanout of the middle row. |
| `scanout` | EagleCount | Time from first row to last row of the scan. |
| `unc_ns`, `lock` | | As in 5.2. |

Glass-to-glass latency for frame `n` is `present(n) − stamp(n)`, an exact
subtraction of two measurements with known uncertainty. It MUST be
reported as data and MUST NOT be fed back into any clock discipline.

---

## 8. Lock

```
enum Lock { Gnss, Ptp, Ntp, Peer, House, Holdover, Free }
```

- `Gnss`, `Ptp`, `Ntp`: disciplined to an absolute reference of that kind.
- `Peer`: disciplined only to another Tukutahi device. Relative lock.
- `House`: following an external genlock or timecode reference that is
  not Eagle-aligned (e.g. a 23.976 house sync). Names remain exact;
  alignment guarantees do not hold.
- `Holdover`: reference lost, coasting on a local oscillator.
  `unc_ns` MUST grow with time.
- `Free`: no discipline. Stamps MUST still be written, and readers MUST
  treat them as an arbitrary clock.

Every mark carries `lock` and `unc_ns`. A stream with `unc_ns` above one
tenth of a period at any mark SHOULD be flagged `degraded` by readers.

---

## 9. Conformance levels

### Level 1: commodity device

- Stamps are measured in software from platform timestamps, corrected by
  a disciplined clock model.
- The sensor, audio codec, and display run on independent oscillators.
  Drift is corrected after the fact (frame repeat/drop, sample slip or
  ASRC).
- Interpolation between marks (5.3) is approximate. `rate_ppm` MUST be
  reported.
- Typical `unc_ns`: 100 µs to 5 ms depending on lock.

### Level 2: single clock tree (Glyph-class hardware)

- One disciplined oscillator (TCXO or better, steered by GNSS 1PPS captured
  in a hardware timer) sources the sensor frame-sync, audio MCLK, and
  display vsync.
- Frame start is timestamped in hardware at the CSI-2 frame-start packet,
  not by software.
- Sensor phase is set by timer compare, so frames land on the grid by
  construction. There is nothing to correct.
- Interpolation between marks is exact; `rate_ppm` is zero by design and
  MAY be omitted.
- Typical `unc_ns`: under 1 µs with GNSS, under 100 ns with hardware PTP.

A stream declares its level. A reader MUST NOT assume Level 2 properties
from a Level 1 stream, whatever its `unc_ns` says.

---

## 10. Audio binding

- Audio is a Tukutahi stream with `rate = 48000/1`, `manawa_period = 48000`.
- Audio is master for lip sync. If audio and video disagree beyond
  tolerance, video is corrected in whole frames; audio is never advanced.
- Tolerance alarms: audio early by more than 15 ms, or late by more than
  45 ms, relative to video.
- A device capturing both MUST derive both streams' stamps from the same
  clock model (Level 1) or the same oscillator (Level 2).

---

## 11. Phase and arrays

`phase` declares an intentional offset of index 0 from the epoch, as a
fraction of a period.

Use: an array of cameras deliberately staggered. Eight Level 2 cameras at
60 fps with phases `0/8 … 7/8` form a 480 fps composite, and every file
says so. Readers MAY interleave such streams by nominal time without any
measurement.

Phase is a declaration, not a measurement. A camera that is merely late
does not declare phase; its stamps show the lateness.

---

## 12. Legacy

The same policy as CIE 1931 xy in VSF RGB: accepted at the boundary,
never used inside.

### 12.1 Rates

- `1000/1001` rates are never native (section 3).
- On import, declare them exactly as rationals with `native = false`.
- Play them on their own rational timeline, or retime to the integer rate
  by a 0.1% speed change (the reverse of film pulldown). Retiming MUST
  resample audio along with video; a bare speed change shifts pitch by
  about 1.7 cents.
- Export to `29.97` / `59.94` MUST remain available, because broadcast
  plants still require them.

### 12.2 Timecode

SMPTE 12M timecode (LTC, VITC, ATC) is an export format.

- HH:MM:SS:FF is rendered from `index` and `rate` on the fly. It is never
  stored.
- Drop-frame is applied only when exporting to a `1000/1001` rate, by the
  standard rule: skip labels `:00` and `:01` at the start of each minute
  except minutes divisible by 10.
- Import of timecode-only material (no absolute reference) MUST set
  `lock = Free` and treat the timecode as a name hint only.

### 12.3 Why 1000/1001 exists

For the record: in 1953, colour NTSC required the 4.5 MHz sound carrier to
be an integer multiple of the line rate to hide the chroma/sound beat.
`286 × 15,750 Hz = 4,504,500 Hz = 1.001 × 4.5 MHz`. They moved the line
rate rather than the sound carrier. Every 23.976 since is that decision.

Note the absurdity (Nick, 2026-09-26): the alternative was nudging the sound carrier from 4.5 MHz to 4.5045 MHz, a 4.5 kHz shift.
The FM sound channel already swings ±25 kHz with ordinary audio, so the installed sets' intercarrier sound would have tracked a 4.5 kHz offset without anyone hearing it.
Instead the frame rate was bent, and seventy years of timecode, drop-frame counting and 1000/1001 rationals inherited a fix for a problem one oscillator could have absorbed.

---

## 13. VSF encoding

Types are sketches; exact tags per the VSF spec.

```
Declaration {
  kind: u8, rate_num: u4, rate_den: u4, phase_num: u4, phase_den: u4,
  manawa_period: u4, level: u3, source: bytes, sensor_rows: u4, native: bool
}
Mark {
  index: i6, stamp: e6, unc_ns: u4, lock: u3, rate_ppm: i4,
  root: bytes32, prev: bytes32, sig: bytes
}
Frame {
  delta: i (exponential width, omitted when 1), hash: bytes32,
  geometry?: Geometry
}
Geometry { exposure: e6, readout: e6 }
Present {
  index: i6, present: e6, scanout: e6, unc_ns: u4, lock: u3
}
```

Eagle counts use VSF `e6`. Names use plain integers. Rates and phases use
plain unsigned integers. No `f` types appear anywhere in this spec.

---

## 14. Validation

A conformant reader MUST reject or flag:

1. A declaration with `native = true` and a rate outside section 3.
2. A declaration with a float in any field.
3. A mark whose `index` is not on the Manawa cadence (section 5.1), unless
   the rate is legacy rational.
4. A mark whose `root` does not match the hashes of the frames since the
   previous mark, or whose `prev` does not match the previous mark.
5. A mark whose `stamp` implies arrival before capture
   (`arrival < stamp − unc_ns`) or later than a configured maximum age.
6. A frame with `delta ≤ 0`.
7. Two marks with the same `index`.
8. A `Level 2` declaration from a `source` not attested as Level 2 hardware
   (attestation mechanism out of scope; see TOKEN).

---

## 15. Conformance tests

1. `index(eagle(n)) == n` for 10^6 random `n` at every native rate and at
   `24000/1001`, `30000/1001`, with zero and non-zero phase.
2. Second-boundary property: for integer rates and zero phase,
   `eagle(n) mod OPS == 0` iff `n mod rate_num == 0`.
3. Audio/video binding: `eagle(800n)` at 48 kHz equals `eagle(n)` at 60 fps
   for all `n`.
4. Interpolation: a Level 1 stream at +80 ppm with one-second marks yields
   per-frame instants within 80 µs of ground truth.
5. Gap declaration: a stream with 3 frames removed decodes with names
   preserved and the gap reported at the right index.
6. Tamper: altering one frame's payload fails validation at the next mark.
7. Two-device clap test (from the Wave LOCK spec) extended to video: a
   strobe flash seen by two Level 1 cameras lands on frames whose stamps
   differ by less than one period.

---

## 16. Open questions

- `EPOCH`: pinned at 20:17:48 TAI to match the Wave spec. 20:17:46 TAI is
  closer to the physical touchdown (1.28 s light time, section 2 note in
  Wave). Decide before any file is written; never move it after.
- Mark signature cost: one signature per second is cheap. Whether
  presentation marks (7.3) also need signing, or are local telemetry only.
- Whether `House` lock (section 8) should carry the house reference's own
  declared rate, so a reader can reconstruct the drift.
- Multi-stream containers: whether a container declares one epoch
  reference for all streams, or each stream repeats it.

---

## Appendix A: derivations

Frame period at 60 fps in oscillations: `OPS / 60 = 23,673,463.77`.
Not an integer, which is why names are rational and stamps are rounded.

Sample period at 48 kHz: `OPS / 48000 = 29,591.83`.

Marks per hour at default cadence: 3600. Signature cost at 64 bytes each:
230 KB per hour of footage.

## Appendix B: reference conversions (Rust)

```rust
pub const OPS: i128 = 1_420_407_826;

/// Round-half-up integer division, correct for negatives.
fn div_round(n: i128, d: i128) -> i128 {
    let q = n.div_euclid(d);
    let r = n.rem_euclid(d);
    q + ((r * 2 >= d) as i128)
}

pub struct Decl { pub rate_num: u32, pub rate_den: u32, pub phase_num: u32, pub phase_den: u32 }

/// Nominal Eagle count of index n.
pub fn eagle_of(d: &Decl, n: i64) -> i64 {
    let num = (n as i128 * d.phase_den as i128 + d.phase_num as i128)
        * d.rate_den as i128 * OPS;
    let den = d.rate_num as i128 * d.phase_den as i128;
    div_round(num, den) as i64
}

/// Nearest index to an Eagle instant e.
pub fn index_of(d: &Decl, e: i64) -> i64 {
    // n = e * rate_num / (rate_den * OPS) - phase_num / phase_den
    let num = e as i128 * d.rate_num as i128 * d.phase_den as i128
        - d.phase_num as i128 * d.rate_den as i128 * OPS;
    let den = d.rate_den as i128 * OPS * d.phase_den as i128;
    div_round(num, den) as i64
}

/// Row-center instant for row r of frame n (rolling shutter).
pub fn row_center(stamp: i64, readout: i64, rows: u32, r: u32) -> i64 {
    if rows <= 1 { return stamp; }
    let num = readout as i128 * (2 * r as i128 - (rows as i128 - 1));
    stamp + div_round(num, 2 * (rows as i128 - 1)) as i64
}
```

Test: `index_of(d, eagle_of(d, n)) == n` for all `n`, all declarations.

---

## Decisions since the draft (maintained by the implementer)

- §16 `EPOCH`: DECIDED — never moved to another instant (Nick 2026-09-25: "we're not changing the epoch"); the definition is 1969-07-20T20:17:48 TAI. Note for implementers: the stamps the fleet mints today count POSIX seconds from the legacy label 1969-07-20T20:17:40 UTC, which runs exactly 29 s behind that definition for any instant since 2017 (vsf `LOCK_MINUS_LEGACY_SECS`); flipping minted stamps onto the TAI scale was DECIDED against (Nick 2026-09-26: "the way it was was fine") — stamps stay on the legacy scale.
- §5.2 `sig`: optional and unused for now (Nick 2026-09-25: decryption is verification too; roll the chain — `root`/`prev` — but don't worry about per-second signatures yet).
- Implementation: `vsf::tukutahi` (vsf/src/tukutahi.rs).
