//! Kept-recording transcode + reader (docs/calls.md — endpoint memory).
//!
//! The live call spools already-ENCODED per-direction mono Opus frames (cheap: ~25 MB/hour, `call/spool.rs`). At KEEP the user wants a real audio FILE with **one channel per participant** (ch0 = local mic, ch1 = remote), so this module transcodes the spool ONCE: decrypt → decode each direction → time-align onto a shared 10 ms grid by eagle-osc → interleave → re-encode one mono Opus stream per channel, in the `PHCALL6` container. Playback ([`crate::call::playback`]) reads it back and sums the channels to mono.
//!
//! **N > 2 (future multi-party — the stubbed "add handle").** Every slot is `nchan` side-by-side MONO packets whatever N is, so multi-party needs no format change — `opus` 0.3.1 has no multistream encoder anyway. Same magic, same reader, one downmix path.
//!
//! **Container:** see [`CONTAINER_MAGIC_V6`] — every slot is `nchan` side-by-side `[len u16 LE][opus]` mono packets. Empty slots are encoded silence — the grid is dense so playback never has to reason about gaps.
//!
//! Transcode is a second lossy Opus generation over the spooled frames (decode-then-re-encode) — the accepted cost of "cheap live spool, rich keep". It is O(call length); run it OFF the UI thread (see `keep_recording`).

use crate::call::spool::{drain_records, Record, SpoolTicket};

/// 10 ms at 48 kHz — the ARCHIVE frame (PHCALL6 slot + KeptStream playback unit). Deliberately unchanged by the 5ms flag day: every previously-kept recording plays without migration.
const FRAME: usize = 480;
/// The LIVE spool frame — 5ms CELT packets since the 2026-09-08 flag day (platform FRAME_SAMPLES). Two input slots fold into one archive slot at transcode.
const FRAME_IN: usize = crate::platform::audio::FRAME_SAMPLES;
/// Input-grid slots per second (5 ms spool packets).
const SLOTS_PER_SEC: i64 = 200;
/// PHCALL6 (waveform flag day 2026-09-11, the envelope pyramid): magic ‖ `[nchan u8][sample_rate u32 LE][base_osc i64 LE][slots u32 LE][env_per_sec u8][env_len u32 LE]` ‖ `nchan × env_len × ENV_COMPONENTS` envelope bytes (channel-major, bucket-major, [amp, r, g, b] each in eighth-stops below full scale, 255 = silence floor) ‖ `slots × nchan` MONO Opus packets (channel order within each slot). `env_len` is variable (0..=ENV_CAP) — the pyramid stores whatever bin count the recording ended on. The envelope is computed at transcode — every frame is decoded here anyway — so no draw path ever decodes audio to show a shape. PHCALL4/5 read support deleted with this flag day (their sub-pitch-period envelopes were the chunking; Nick: no backward compat, old previews looked bad).
pub const CONTAINER_MAGIC_V6: &[u8; 8] = b"PHCALL6\0";
/// Per-channel archive bitrate: CELT fullband is transparent for speech well below this; a two-party hour is ~115 MB.
const ARCHIVE_KBPS: i32 = 128_000;
/// Envelope components per bin per channel (Nick 2026-09-12, "three pyramids"): [0] power `x²`, [1] first-difference detail power `(n₀−n₁)²`, [2] second-level Haar detail power `((n₀+n₁)−(n₂+n₃))²`. Three channels of a u8 normalized tensor; how they colour the card is worked out downstream, once the aliasing is right.
pub const ENV_COMPONENTS: usize = 3;
/// Envelope pyramid capacity (Nick 2026-09-11: "a bin that's 2^17 wide that triggers a resize to downsample all by 2"): the working buffer is this many bins per channel, and a recording stores whatever count it ends on (0..=ENV_CAP), so `env_len` is genuinely variable now.
pub const ENV_CAP: usize = 1 << 17;
/// Starting samples-per-bin exponent: ONE sample — every completed wave of at least [`ENV_MIN_SHARE_SAMPLES`] lands between 2^16 and 2^17 bins after the folds. Sub-pitch ripple in the stored bins is fine now: the render's cumulative stack averages ~a gross of bins into every pixel column, so the ripple dies at display instead of at capture.
const ENV_S0_LOG2: u32 = 0;
/// A wave shorter than this many samples (~1.4 s) mints no wave.env blob — cheaper for the receiver to derive the envelope from the audio it fetches anyway.
pub const ENV_MIN_SHARE_SAMPLES: usize = 1 << 16;

/// Resample per-slot LINEAR values to `out_len` buckets: a box mean when folding down, linear interpolation between slot centres when stretching up — the "bilinear" half of the pipeline, on the recording's own grid.
pub fn resample_linear(slots: &[f32], out_len: usize) -> Vec<f32> {
    let n = slots.len();
    if n == 0 || out_len == 0 {
        return vec![0.0; out_len];
    }
    (0..out_len)
        .map(|b| {
            let s0 = b as f32 * n as f32 / out_len as f32;
            let s1 = (b + 1) as f32 * n as f32 / out_len as f32;
            if s1 - s0 >= 1.0 {
                // Fold: mean over the slots this bucket spans, edge slots weighted by their overlap.
                let (mut acc, mut cov) = (0f32, 0f32);
                let mut i = s0.floor() as usize;
                while (i as f32) < s1 && i < n {
                    let c = (s1.min(i as f32 + 1.0) - s0.max(i as f32)).max(0.0);
                    acc += slots[i] * c;
                    cov += c;
                    i += 1;
                }
                if cov > 0.0 { acc / cov } else { 0.0 }
            } else {
                // Stretch: interpolate between the two nearest slot centres.
                let c = ((s0 + s1) * 0.5 - 0.5).clamp(0.0, (n - 1) as f32);
                let i0 = c.floor() as usize;
                let i1 = (i0 + 1).min(n - 1);
                let t = c - i0 as f32;
                slots[i0] * (1.0 - t) + slots[i1] * t
            }
        })
        .collect()
}

/// Header byte kept for the format's shape: 0 = no embedded envelope (the wave.env exchange, 2026-09-12); the old per-second cadence is gone.
pub const ENV_PER_SEC: usize = 0;

/// Per-channel state for the two decimated detail streams: the previous sample (for `n₀−n₁` at every odd index) and the previous pair-sum (for `(n₀+n₁)−(n₂+n₃)` at every fourth).
struct HaarState {
    s1: i64,
    p2: i64,
}

/// The streaming envelope accumulator (Nick 2026-09-11: "a bin that's 2^17 wide that triggers a resize to downsample all by 2 to keep the total size under 2^17").
/// Fixed integer samples-per-bin starting at 2^ENV_S0_LOG2; when a sample's bin index would pass ENV_CAP, adjacent bin pairs merge (sums and counts ADD, so the fold is bit-exact) and the bin width doubles.
/// u64 sums of squares: a full-scale square wave needs ~25 hours in one bin to overflow, and unlike f64 the sums never round, so any resize history yields identical bytes.
/// Needs no total length up front — the same structure can feed a live in-call band later.
struct EnvPyramid {
    nchan: usize,
    cap: usize,
    s_log2: u32,
    sumsq: Vec<u64>,
    counts: Vec<u64>,
    haar: Vec<HaarState>,
    total_samples: usize,
}

impl EnvPyramid {
    fn new(nchan: usize) -> Self {
        Self::with_geometry(nchan, ENV_CAP, ENV_S0_LOG2)
    }
    /// The test seam: a small capacity forces folds without hundreds of millions of pushes, and a raised s0 computes the direct binning a folded run must equal.
    fn with_geometry(nchan: usize, cap: usize, s0_log2: u32) -> Self {
        EnvPyramid {
            nchan,
            cap,
            s_log2: s0_log2,
            sumsq: vec![0u64; nchan * cap * ENV_COMPONENTS],
            counts: vec![0u64; nchan * cap * ENV_COMPONENTS],
            haar: (0..nchan).map(|_| HaarState { s1: 0, p2: 0 }).collect(),
            total_samples: 0,
        }
    }
    /// Merge adjacent bin pairs and double the bin width — pure addition, bit-exact, any number of times.
    fn fold(&mut self) {
        let k = ENV_COMPONENTS;
        for ch in 0..self.nchan {
            let base = ch * self.cap * k;
            for i in 0..self.cap / 2 {
                for c in 0..k {
                    self.sumsq[base + i * k + c] = self.sumsq[base + 2 * i * k + c] + self.sumsq[base + (2 * i + 1) * k + c];
                    self.counts[base + i * k + c] = self.counts[base + 2 * i * k + c] + self.counts[base + (2 * i + 1) * k + c];
                }
            }
            for v in &mut self.sumsq[base + self.cap / 2 * k..base + self.cap * k] {
                *v = 0;
            }
            for v in &mut self.counts[base + self.cap / 2 * k..base + self.cap * k] {
                *v = 0;
            }
        }
        self.s_log2 += 1;
    }
    /// One sample at its absolute archive index: amplitude squares in every sample; the tree details square in on the odd boundary of their level, binned at the emitting sample's bin.
    #[inline]
    fn push(&mut self, ch: usize, abs_idx: usize, x: i16) {
        while (abs_idx >> self.s_log2) >= self.cap {
            self.fold();
        }
        self.total_samples = self.total_samples.max(abs_idx + 1);
        let k = ENV_COMPONENTS;
        let base = (ch * self.cap + (abs_idx >> self.s_log2)) * k;
        let xi = x as i64;
        self.sumsq[base] += (xi * xi) as u64;
        self.counts[base] += 1;
        // The two detail streams, raw Haar (no mean scaling — the per-channel peak normalization at finish washes any constant factor out): d1 on every odd sample, d2 on every fourth.
        let h = &mut self.haar[ch];
        if abs_idx & 1 == 0 {
            h.s1 = xi;
            return;
        }
        let d1 = h.s1 - xi;
        self.sumsq[base + 1] += (d1 * d1) as u64;
        self.counts[base + 1] += 1;
        let pair = h.s1 + xi;
        if (abs_idx >> 1) & 1 == 0 {
            h.p2 = pair;
            return;
        }
        let d2 = h.p2 - pair;
        self.sumsq[base + 2] += (d2 * d2) as u64;
        self.counts[base + 2] += 1;
    }
    /// Reduce ONE channel to the shareable form (Nick 2026-09-12, "3 channel u8 normalized tensor"): per bin per component the MEAN POWER (`sumsq/count`), each component normalized to its own peak over the recording, quantized to u8 by STOCHASTIC rounding — the dither noise carries sub-LSB signal into the render's per-pixel averages, so 8 bits hold the full dynamic range statistically. Returns (bins, per-component peak power relative to full scale, planar `[3][bins]` bytes); None for an empty channel.
    fn finish_u8(&self, ch: usize) -> Option<(usize, [u64; 3], Vec<u8>)> {
        let k = ENV_COMPONENTS;
        let s = 1usize << self.s_log2;
        let bins = self.total_samples.div_ceil(s).min(self.cap);
        if bins == 0 {
            return None;
        }
        let mean = |bin: usize, c: usize| -> f64 {
            let i = (ch * self.cap + bin) * k + c;
            if self.counts[i] == 0 { 0.0 } else { self.sumsq[i] as f64 / self.counts[i] as f64 }
        };
        let mut peaks = [0f64; ENV_COMPONENTS];
        for c in 0..k {
            for bin in 0..bins {
                let v = mean(bin, c);
                if v > peaks[c] {
                    peaks[c] = v;
                }
            }
        }
        // Deterministic dither (splitmix64 on the flat index): the same recording always quantizes to the same bytes, and the ensemble is uniform so the mean is unbiased.
        let dither01 = |i: u64| -> f64 {
            let mut z = i.wrapping_add(0x9E37_79B9_7F4A_7C15);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            (z ^ (z >> 31)) as f64 / (u64::MAX as f64)
        };
        // A byte is 1/256 of the peak (256, not 255 — integer maths floors; the peak bin lands on 256 and clamps into the top step).
        let mut data = vec![0u8; k * bins];
        for c in 0..k {
            for bin in 0..bins {
                let q = if peaks[c] > 0.0 { (mean(bin, c) / peaks[c] * 256.0).clamp(0.0, 256.0) } else { 0.0 };
                let f = q.floor();
                let up = (q - f) > dither01((c * self.cap + bin) as u64);
                data[c * bins + bin] = (f as u64 + up as u64).min(255) as u8;
            }
        }
        // Peak power relative to full scale in Q48 — an exact integer at rest; decode is multiplication and shifts only.
        const FS_POWER: f64 = 32768.0 * 32768.0;
        let q48 = |v: f64| ((v / FS_POWER) * (1u64 << 48) as f64).round() as u64;
        Some((bins, [q48(peaks[0]), q48(peaks[1]), q48(peaks[2])], data))
    }
}

/// A finished transcode: the sealed container plus what the wave card needs without opening it.
pub struct Transcoded {
    pub container: Vec<u8>,
    /// Our channel's shareable wave.env VSF file (None for a short wave — the receiver derives from audio).
    pub env: Option<Vec<u8>>,
    /// Whole seconds of audio (archive slots ÷ 100).
    pub secs: u32,
}

/// A kept blob as stored: content hash, byte size, and the card fields.
pub struct Kept {
    pub hash: [u8; 32],
    pub size: u64,
    /// Our channel's wave.env VSF file, stored beside the recording: (content hash, bytes). None for a short wave.
    pub env: Option<([u8; 32], Vec<u8>)>,
    pub secs: u32,
}

fn osc_to_slot(osc: i64, base: i64) -> i64 {
    let ops = vsf::OSCILLATIONS_PER_SECOND as i64;
    if ops <= 0 {
        return 0;
    }
    let d = (osc - base).max(0);
    (d * SLOTS_PER_SEC + ops / 2) / ops
}

/// Bucket drained spool records into a dense per-channel × per-slot grid of encoded frames. Returns `(nchan, grid[chan][slot] = Option<opus>)`. `base_osc` is the earliest frame across all channels; every frame lands at `round((osc-base)*100/OSC_PER_SEC)`. A collision (two frames of one channel rounding to the same slot — osc jitter under 5 ms) keeps the last; gaps (remote packet loss) stay `None` = encoded silence at read time.
/// One spooled frame: an Opus packet, or raw little-endian i16 PCM from the plaid rung (spool.rs RAW_FLAG).
#[derive(Clone)]
enum Cell {
    Opus(Vec<u8>),
    /// Raw little-endian i16 PCM and the gain to apply at decode (1.0 for a wire cell; the live path's applied gain for a raw-mic cell, so the keep carries the duck and gate contour the peer heard while working from the raw source).
    Pcm(Vec<u8>, f32),
}

fn grid_from_records(records: &[Record]) -> Option<(usize, Vec<Vec<Option<Cell>>>)> {
    if records.is_empty() {
        return None;
    }
    let base = records.iter().filter(|r| !r.is_fill()).map(|r| r.osc).min().or_else(|| records.iter().map(|r| r.osc).min())?;
    let nchan = records.iter().map(|r| r.index()).max()? + 1;
    // RAW MIC OUTRANKS THE WIRE COPY (2026-09-10): a channel that has PROC records (the mic as captured) is built from them alone; its wire copies (ducked, gated, coded) are what the peer heard, not what the archive should keep. Spools without PROC records fall back to the wire copies as before.
    let has_raw: Vec<bool> = (0..nchan).map(|c| records.iter().any(|r| r.index() == c && r.is_raw_mic())).collect();
    let keep = |r: &Record| !(has_raw[r.index()] && !r.is_raw_mic());
    // LATTICE SLOTTING (field 2026-09-08, "super garbled" preview): frames are stamped at DRAIN, in 1ms engine-loop bursts — adjacent 5ms frames carry near-identical stamps, and slotting each by its own stamp collided them ("collision keeps the later one" = half the frames dropped). So each channel keeps a lattice: a frame within `reanchor` of the expected next stamp takes the NEXT slot; a real gap (loss, a stall) re-anchors on the stamp.
    let ops = vsf::OSCILLATIONS_PER_SECOND as i64;
    let reanchor = ops / 20; // 50ms — ten slots; burst jitter is ±ms, real gaps are bigger
    let mut lat: Vec<Option<(i64, i64)>> = vec![None; nchan]; // per channel: (next_slot, expected_osc)
    // SEQ LATTICE (recording fills, 2026-09-10): a record that names its wire window is slotted by that — the next window's first frame sits exactly (windows between × the previous window's frame count) slots on, so a LOST window leaves its hole in the grid for the fill to land in (the stamp lattice would swallow a 5–40 ms hole as burst jitter). The stamp still re-anchors the ruler when seq and stamp disagree by more than the lattice tolerance (a stall, a rung change across a gap).
    let mut seq_lat: Vec<Option<(u32, i64, i64)>> = vec![None; nchan]; // per channel: (last seq, its first-frame slot, its frame count so far)
    let mut slotted: Vec<(usize, usize, Cell)> = Vec::with_capacity(records.len());
    let mut max_slot = 0usize;
    // Per channel: window seq → (grid slot of its first frame, frames in the window) for the records that ARRIVED live — the ruler the fills are placed against.
    let mut arrived: Vec<std::collections::BTreeMap<u32, (usize, usize)>> = vec![Default::default(); nchan];
    let cell_of = |r: &Record| if r.is_raw() { Cell::Pcm(r.bytes.clone(), r.proc.map_or(1.0, |(g, _)| g as f32 / 256.0)) } else { Cell::Opus(r.bytes.clone()) };
    let reanchor_slots = reanchor * SLOTS_PER_SEC / ops;
    for r in records.iter().filter(|r| !r.is_fill() && keep(r)) {
        let c = r.index();
        let by_stamp = match lat[c] {
            Some((next, expected)) if (r.osc - expected).abs() < reanchor => next,
            _ => osc_to_slot(r.osc, base),
        };
        let slot = match (r.seq, seq_lat[c]) {
            (Some((seq, wslot)), Some((lseq, lslot0, ln))) if seq >= lseq => {
                let by_seq = if seq == lseq { lslot0 + wslot as i64 } else { lslot0 + (seq - lseq) as i64 * ln.max(1) + wslot as i64 };
                if (by_seq - by_stamp).abs() < reanchor_slots.max(2) { by_seq } else { by_stamp }
            }
            _ => by_stamp,
        };
        let slot_u = slot.max(0) as usize;
        lat[c] = Some((slot + 1, base + (slot + 1) * ops / SLOTS_PER_SEC));
        if let Some((seq, wslot)) = r.seq {
            seq_lat[c] = match seq_lat[c] {
                Some((lseq, lslot0, ln)) if lseq == seq => Some((seq, lslot0, ln + 1)),
                _ => Some((seq, slot - wslot as i64, 1)),
            };
            let e = arrived[c].entry(seq).or_insert((slot_u.saturating_sub(wslot as usize), 0));
            e.1 += 1;
        }
        max_slot = max_slot.max(slot_u);
        slotted.push((c, slot_u, cell_of(r)));
    }
    // FILLS (recording fills, 2026-09-10): a window the peer served after the fact has no honest arrival stamp, but it has a seq — the same seq its neighbours carried. It sits exactly where the ruler says: after the nearest earlier arrived window (its slot + the windows between × that window's frame count), else before the nearest later one. A fill never overwrites a frame that arrived.
    let mut fills: Vec<(usize, usize, Cell)> = Vec::new();
    let mut fill_frames: std::collections::HashMap<(usize, u32), usize> = Default::default();
    for r in records.iter().filter(|r| r.is_fill()) {
        if let Some((seq, _)) = r.seq {
            *fill_frames.entry((r.index(), seq)).or_default() += 1;
        }
    }
    for r in records.iter().filter(|r| r.is_fill()) {
        let (Some((seq, wslot)), c) = (r.seq, r.index()) else {
            continue;
        };
        let prev = arrived[c].range(..seq).next_back().map(|(s, v)| (*s, *v));
        let next = arrived[c].range(seq + 1..).next().map(|(s, v)| (*s, *v));
        let own = fill_frames.get(&(c, seq)).copied().unwrap_or(1).max(1);
        let slot = match (prev, next) {
            (Some((ps, (pslot, pn))), n) if n.map_or(true, |(ns, _)| seq - ps <= ns - seq) => pslot as i64 + (seq - ps) as i64 * pn.max(1) as i64 + wslot as i64,
            (_, Some((ns, (nslot, _)))) => nslot as i64 - (ns - seq) as i64 * own as i64 + wslot as i64,
            (Some((ps, (pslot, pn))), None) => pslot as i64 + (seq - ps) as i64 * pn.max(1) as i64 + wslot as i64,
            (None, None) => continue,
        };
        if slot < 0 {
            continue;
        }
        max_slot = max_slot.max(slot as usize);
        fills.push((c, slot as usize, cell_of(r)));
    }
    let mut grid: Vec<Vec<Option<Cell>>> = vec![vec![None; max_slot + 1]; nchan];
    for (c, slot, cell) in slotted {
        grid[c][slot] = Some(cell);
    }
    for (c, slot, cell) in fills {
        if grid[c][slot].is_none() {
            grid[c][slot] = Some(cell);
        }
    }
    Some((nchan, grid))
}

fn mono_decoder() -> Option<opus::Decoder> {
    opus::Decoder::new(48_000, opus::Channels::Mono).ok()
}

/// Decode one channel's slot (or silence) with its running decoder, at the caller's frame size — `FRAME_IN` for live-spool input (transcode + preview), `FRAME` for PHCALL6 archive playback. Silence slots do NOT touch the decoder (no frame was ever encoded there — the gap is genuine), so the next real frame decodes in step.
fn decode_slot_n(dec: &mut opus::Decoder, cell: &Option<Cell>, frame: usize) -> Vec<i16> {
    match cell {
        Some(Cell::Opus(opus)) => {
            let mut pcm = vec![0i16; frame];
            match dec.decode(opus, &mut pcm, false) {
                Ok(n) if n == frame => pcm,
                _ => vec![0i16; frame],
            }
        }
        // A plaid frame is already the samples; a wrong-sized one is silence, never a guess.
        Some(Cell::Pcm(raw, gain)) if raw.len() == frame * 2 => raw
            .chunks_exact(2)
            .map(|c| {
                let s = i16::from_le_bytes([c[0], c[1]]);
                if (*gain - 1.0).abs() < 0.002 { s } else { (s as f32 * gain).clamp(-32768.0, 32767.0) as i16 }
            })
            .collect(),
        Some(Cell::Pcm(..)) | None => vec![0i16; frame],
    }
}

/// KEEP with transcode → one mono Opus stream per channel in the `PHCALL6` container, stored as a content-addressed blob. Returns (content_hash, size); consumes the ticket (dropping it crypto-shreds the spool key either way); removes the spool file on success. `None` = nothing recorded (treat keep as delete) or a codec init failure.
pub fn finalize_nchannel(ticket: SpoolTicket, identity_seed: &[u8; 32]) -> Option<Kept> {
    let records = drain_records(&ticket)?;
    let Some(t) = build_container(&records) else {
        crate::call::spool::shred(ticket);
        return None;
    };
    let hash = *blake3::hash(&t.container).as_bytes();
    let size = t.container.len() as u64;
    // Chunked past BLOB_CHUNK_SIZE (2026-09-11): a 13-minute wave is 26 MB, and one whole PT transfer of that never reached the desktop on any leg — chunks replicate a piece at a time with resume, like every other attachment.
    crate::storage::blob_store_any(identity_seed, &hash, &t.container).ok()?;
    // The envelope blob rides beside the recording under its own hash — small, so it lands at the far end long before the audio.
    let env = t.env.and_then(|e| {
        let eh = *blake3::hash(&e).as_bytes();
        crate::storage::blob_store_any(identity_seed, &eh, &e).ok().map(|_| (eh, e))
    });
    let _ = std::fs::remove_file(&ticket.path);
    Some(Kept { hash, size, env, secs: t.secs })
}

/// The transcode core: drained spool records → a `PHCALL6` container (bytes). Split from [`finalize_nchannel`] so it's testable without the storage/vault layer. `None` = nothing recorded or a codec init failure.
pub(crate) fn build_container(records: &[Record]) -> Option<Transcoded> {
    let (nchan, grid) = grid_from_records(records)?;
    let slots = grid[0].len();
    let slots_out = slots.div_ceil(2);
    let base = records.iter().filter(|r| !r.is_fill()).map(|r| r.osc).min().unwrap_or(0);
    // Envelope: every decoded sample streams into the EnvPyramid at its absolute archive index — fixed integer bins from birth, fold-by-2 on overflow, no total needed up front (see the struct doc). The Haar states carry across slots so the tree sees one continuous signal per channel.
    let mut pyramid = EnvPyramid::new(nchan);

    // Packets first, header after: the header carries the envelope, which the packet loop produces.
    let mut container = Vec::with_capacity(slots * 48);

    let mut decs: Vec<opus::Decoder> = (0..nchan).map(|_| mono_decoder()).collect::<Option<_>>()?;
    let mut pkt = vec![0u8; 4000];
    let write_pkt = |container: &mut Vec<u8>, enc: &[u8]| {
        container.extend_from_slice(&(enc.len() as u16).to_le_bytes());
        container.extend_from_slice(enc);
    };

    // ONE MONO STREAM PER PARTY (since PHCALL5, 2026-09-10): the local channel is a single lossy generation from the raw mic, every channel at the same transparent bitrate; nchan side-by-side mono packets per slot.
    {
        // N > 2 fallback: nchan side-by-side MONO packets per slot (no multistream Opus in this crate).
        let mut encs: Vec<opus::Encoder> = (0..nchan)
            .map(|_| {
                let mut e = opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Audio).ok()?;
                let _ = e.set_vbr(true);
                let _ = e.set_bitrate(opus::Bitrate::Bits(ARCHIVE_KBPS));
                // Complexity 6 of 10: about half the CPU of the default at 128 kbps with no audible cost — a 13-minute wave is 160k encodes on a phone that may already be dozing (Emma's keep took 53 minutes at the default, 2026-09-11).
                let _ = e.set_complexity(6);
                Some(e)
            })
            .collect::<Option<_>>()?;
        for slot_out in 0..slots_out {
            for ch in 0..nchan {
                let mut pcm = vec![0i16; FRAME];
                for half in 0..2 {
                    let slot_in = slot_out * 2 + half;
                    let p = if slot_in < slots {
                        decode_slot_n(&mut decs[ch], &grid[ch][slot_in], FRAME_IN)
                    } else {
                        vec![0i16; FRAME_IN]
                    };
                    pcm[half * FRAME_IN..half * FRAME_IN + FRAME_IN].copy_from_slice(&p);
                }
                for (i, &s) in pcm.iter().enumerate() {
                    pyramid.push(ch, slot_out * FRAME + i, s);
                }
                let n = encs[ch].encode(&pcm, &mut pkt).ok()?;
                write_pkt(&mut container, &pkt[..n]);
            }
        }
    }

    if container.is_empty() {
        return None;
    }
    // The container carries NO envelope since the wave.env exchange (2026-09-12): env_len writes 0 and the envelope lives as a standalone VSF tensor blob per party. The header keeps the field so the layout is unchanged.
    let mut out = Vec::with_capacity(CONTAINER_MAGIC_V6.len() + 22 + container.len());
    out.extend_from_slice(CONTAINER_MAGIC_V6);
    out.push(nchan as u8);
    out.extend_from_slice(&48_000u32.to_le_bytes());
    out.extend_from_slice(&base.to_le_bytes());
    out.extend_from_slice(&(slots_out as u32).to_le_bytes());
    out.push(ENV_PER_SEC as u8);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&container);
    // Our OWN channel's shareable envelope (ch0 = the raw mic): minted here where the samples were just decoded anyway; a short wave (< ENV_MIN_SHARE_SAMPLES) ships none by design.
    let env = if pyramid.total_samples >= ENV_MIN_SHARE_SAMPLES {
        pyramid.finish_u8(0).map(|(bins, peak_q48, data)| crate::call::wave_env::write(48_000, 1u32 << pyramid.s_log2, bins, peak_q48, &data))
    } else {
        None
    };
    Some(Transcoded { container: out, env, secs: (slots_out / 100) as u32 })
}

/// A decoded recording as a stream of interleaved `FRAME × nchan` i16 frames. Bounded memory: the compressed container stays in RAM (~tens of MB/hour) and each 10 ms frame decodes on demand via [`Self::next_frame`] — never the whole PCM at once (a stereo hour is ~700 MB decoded).
pub struct KeptStream {
    pub nchan: usize,
    /// Total playable frames (archive slots for a kept blob, grid slots for a live-spool preview) — the scrub bar's denominator.
    pub total: usize,
    /// The fine envelope from the container header (`nchan × env_len × ENV_COMPONENTS`, eighth-stops) — empty for a live-spool preview.
    pub envelope: Vec<u8>,
    pub env_per_sec: u8,
    inner: Inner,
}

enum Inner {
    /// PHCALL6: nchan mono decoders, `nchan` packets per slot.
    Multi {
        bytes: Vec<u8>,
        cur: usize,
        decs: Vec<opus::Decoder>,
    },
    /// Live-spool grid (the Ended-screen PREVIEW path): 5ms input slots, slot-iterated + interleaved on the fly.
    Grid {
        grid: Vec<Vec<Option<Cell>>>,
        decs: Vec<opus::Decoder>,
        slot: usize,
    },
}

/// Open a kept-call blob for playback — `PHCALL6` only (PHCALL1-5 read support deleted with their flag days, no backwards compat — unknown magic is unknown magic). `None` on unknown magic or codec init failure.
pub fn open_blob(bytes: &[u8]) -> Option<KeptStream> {
    if bytes.len() < 8 + 22 || &bytes[..8] != CONTAINER_MAGIC_V6 {
        return None;
    }
    let nchan = bytes[8] as usize;
    // header: [nchan u8][rate u32][base i64][slots u32][env_per_sec u8][env_len u32] = 22 bytes after magic; envelope then packets follow.
    let total = u32::from_le_bytes(bytes[8 + 13..8 + 17].try_into().ok()?) as usize;
    let env_per_sec = bytes[8 + 17];
    let env_len = u32::from_le_bytes(bytes[8 + 18..8 + 22].try_into().ok()?) as usize;
    let env_end = 8 + 22 + nchan * env_len * ENV_COMPONENTS;
    if bytes.len() < env_end || nchan == 0 {
        return None;
    }
    let envelope = bytes[8 + 22..env_end].to_vec();
    let body = bytes[env_end..].to_vec();
    let inner = Inner::Multi {
        bytes: body,
        cur: 0,
        decs: (0..nchan).map(|_| mono_decoder()).collect::<Option<_>>()?,
    };
    Some(KeptStream { nchan, total, envelope, env_per_sec, inner })
}

/// Build a playable stream directly from drained spool records — the Ended-screen PREVIEW path, so Play works before Keep finalizes a blob.
pub(crate) fn stream_from_records(records: &[Record]) -> Option<KeptStream> {
    let (nchan, grid) = grid_from_records(records)?;
    Some(KeptStream {
        nchan,
        total: grid[0].len(),
        envelope: Vec::new(),
        env_per_sec: 0,
        inner: Inner::Grid {
            grid,
            decs: (0..nchan).map(|_| mono_decoder()).collect::<Option<_>>()?,
            slot: 0,
        },
    })
}

/// Frames decoded-and-discarded ahead of a seek target so the stateful codec has converged by the time audible output starts (Opus pre-roll is ~80 ms; this is generous).
const SEEK_PRIME: usize = 8;

impl KeptStream {
    /// Position the stream at archive slot `slot` (clamped to the end): packets before the target are WALKED by their length prefix, never decoded — a two-hour seek is a byte scan, not a two-hour decode — then `SEEK_PRIME` frames prime the decoder. The tap-to-seek path.
    pub fn seek(&mut self, slot: usize) {
        let slot = slot.min(self.total);
        let start = slot.saturating_sub(SEEK_PRIME);
        let nchan = self.nchan;
        match &mut self.inner {
            Inner::Multi { bytes, cur, decs } => {
                *cur = 0;
                for _ in 0..start * nchan {
                    if read_pkt(bytes, cur).is_none() {
                        return;
                    }
                }
                for d in decs.iter_mut() {
                    let _ = d.reset_state();
                }
            }
            Inner::Grid { slot: s, decs, .. } => {
                *s = start;
                for d in decs.iter_mut() {
                    let _ = d.reset_state();
                }
            }
        }
        for _ in start..slot {
            if self.next_frame().is_none() {
                break;
            }
        }
    }

    /// The next interleaved `FRAME × nchan` i16 frame, or `None` at end of stream. A decode failure inside the stream yields silence for that frame rather than ending playback early.
    pub fn next_frame(&mut self) -> Option<Vec<i16>> {
        let nchan = self.nchan;
        match &mut self.inner {
            Inner::Multi { bytes, cur, decs } => {
                let mut out = vec![0i16; FRAME * nchan];
                for ch in 0..nchan {
                    let Some(opus) = read_pkt(bytes, cur) else {
                        if ch == 0 {
                            return None; // clean end on a slot boundary
                        }
                        break; // truncated tail mid-slot — emit what we have
                    };
                    let mut pcm = vec![0i16; FRAME];
                    let _ = decs[ch].decode(opus, &mut pcm, false);
                    for (i, &s) in pcm.iter().enumerate() {
                        out[i * nchan + ch] = s;
                    }
                }
                Some(out)
            }
            Inner::Grid { grid, decs, slot } => {
                if *slot >= grid[0].len() {
                    return None;
                }
                let mut out = vec![0i16; FRAME_IN * nchan];
                for ch in 0..nchan {
                    // The grid is live spool (preview) — 5ms packets.
                    let pcm = decode_slot_n(&mut decs[ch], &grid[ch][*slot], FRAME_IN);
                    for (i, &s) in pcm.iter().enumerate() {
                        out[i * nchan + ch] = s;
                    }
                }
                *slot += 1;
                Some(out)
            }
        }
    }
}

/// Derive every channel's envelope from a held recording — the fallback for short waves (no blob shipped by design), the far channel before its wave.env lands, and every pre-exchange recording. One full decode, off the UI thread; ~real-time-x50 on a laptop.
pub fn envelopes_from_blob(bytes: &[u8]) -> Option<Vec<crate::call::wave_env::WaveEnv>> {
    let mut ks = open_blob(bytes)?;
    let nchan = ks.nchan;
    let mut pyr = EnvPyramid::new(nchan);
    let mut idx = 0usize;
    while let Some(f) = ks.next_frame() {
        for i in 0..FRAME {
            for ch in 0..nchan {
                pyr.push(ch, idx + i, f[i * nchan + ch]);
            }
        }
        idx += FRAME;
    }
    let spb = 1u32 << pyr.s_log2;
    let envs: Vec<crate::call::wave_env::WaveEnv> = (0..nchan)
        .filter_map(|ch| {
            pyr.finish_u8(ch).map(|(bins, peak_q48, data)| crate::call::wave_env::WaveEnv {
                bins,
                sample_rate: 48_000,
                samples_per_bin: spb,
                peak_q48,
                data: std::sync::Arc::new(data),
            })
        })
        .collect();
    (envs.len() == nchan).then_some(envs)
}

/// Read one `[len u16 LE][bytes]` packet, advancing `cur`. `None` at end / on a truncated length prefix.
fn read_pkt<'a>(bytes: &'a [u8], cur: &mut usize) -> Option<&'a [u8]> {
    if *cur + 2 > bytes.len() {
        return None;
    }
    let len = u16::from_le_bytes(bytes[*cur..*cur + 2].try_into().ok()?) as usize;
    *cur += 2;
    if *cur + len > bytes.len() {
        return None;
    }
    let p = &bytes[*cur..*cur + len];
    *cur += len;
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcode_round_trips_to_stereo() {
        // Build two directions of encoded frames, transcode to a PHCALL6 container, reopen, assert stereo + audible. No storage/vault (build_container is the transcode core).
        let mut enc =
            opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Audio).unwrap();
        let mut buf = vec![0u8; 4000];
        let ops = vsf::OSCILLATIONS_PER_SECOND as i64;
        let mut records: Vec<Record> = Vec::new();
        for i in 0..20i64 {
            // A tone so decode is non-zero; both directions, 5 ms apart (the live spool cadence) — 20 input slots fold to 10 archive slots.
            let tone: Vec<i16> = (0..FRAME_IN)
                .map(|s| ((s as f32 * 0.1).sin() * 4000.0) as i16)
                .collect();
            let n = enc.encode(&tone, &mut buf).unwrap();
            let osc = i * (ops / 200);
            records.push(Record { chan: 0, osc, seq: None, proc: None, bytes: buf[..n].to_vec() });
            records.push(Record { chan: 1, osc, seq: None, proc: None, bytes: buf[..n].to_vec() });
        }
        let t = build_container(&records).unwrap();
        // 10 archive slots = 4800 samples, under ENV_MIN_SHARE_SAMPLES — a short wave ships no envelope by design; the receiver derives it from the audio.
        assert!(t.env.is_none(), "a short wave must not mint a wave.env");
        let container = t.container;
        assert_eq!(&container[..8], CONTAINER_MAGIC_V6);
        let envs = envelopes_from_blob(&container).expect("derive from audio");
        assert_eq!(envs.len(), 2);
        assert!(envs[0].bins > 0 && envs[0].data.iter().any(|&b| b > 0), "derived envelope shows no signal");
        let mut ks = open_blob(&container).unwrap();
        assert_eq!(ks.nchan, 2);
        assert_eq!(ks.env_per_sec as usize, ENV_PER_SEC);
        assert!(ks.envelope.is_empty(), "the container carries no embedded envelope since the wave.env exchange");
        let mut frames = 0;
        let mut energy = 0i64;
        while let Some(f) = ks.next_frame() {
            assert_eq!(f.len(), FRAME * 2);
            energy += f.iter().map(|&s| s.unsigned_abs() as i64).sum::<i64>();
            frames += 1;
        }
        assert!(frames >= 8, "expected ~10 slots, got {frames}");
        assert!(energy > 0, "decoded audio was pure silence");
        // Seek: positioning near the end leaves only the tail to play, and a seek past the end plays nothing.
        let mut ks2 = open_blob(&container).unwrap();
        let total = ks2.total;
        ks2.seek(total - 2);
        let mut tail = 0;
        while ks2.next_frame().is_some() {
            tail += 1;
        }
        assert_eq!(tail, 2, "seek to total-2 should leave two frames");
        let mut ks3 = open_blob(&container).unwrap();
        ks3.seek(total + 50);
        assert!(ks3.next_frame().is_none());
    }

    #[test]
    fn pyramid_matches_direct_binning() {
        // The exactness claim: samples streamed through fold-by-2 resizes yield the SAME sums and counts as binning directly at the final width — pure u64 addition, no rounding history.
        let cap = 8usize;
        let s0 = 10u32;
        let n = 5 * cap * (1usize << s0);
        let mut streamed = EnvPyramid::with_geometry(1, cap, s0);
        for i in 0..n {
            let x = (((i * 37 + 11) % 3001) as i32 - 1500) as i16;
            streamed.push(0, i, x);
        }
        // 40 starting bins in a cap of 8 forces three folds: the direct run starts at the folded width and never resizes.
        assert_eq!(streamed.s_log2, s0 + 3);
        let mut direct = EnvPyramid::with_geometry(1, cap, s0 + 3);
        for i in 0..n {
            let x = (((i * 37 + 11) % 3001) as i32 - 1500) as i16;
            direct.push(0, i, x);
        }
        assert_eq!(streamed.sumsq, direct.sumsq);
        assert_eq!(streamed.counts, direct.counts);
        let (a_bins, a_peaks, a_data) = streamed.finish_u8(0).unwrap();
        let (b_bins, b_peaks, b_data) = direct.finish_u8(0).unwrap();
        assert_eq!(a_bins, b_bins);
        assert_eq!(a_peaks, b_peaks);
        assert_eq!(a_data, b_data);
    }

    #[test]
    fn detail_channels_separate_octaves() {
        // The two detail streams are octave-exclusive: DC excites neither, a period-2 wave only d1, a period-4 wave only d2, and a slow wave (period 8, constant across every aligned quad) neither.
        let band_energy = |signal: &dyn Fn(usize) -> i16| -> [u64; 2] {
            let mut p = EnvPyramid::with_geometry(1, 16, 10);
            for i in 0..4096 {
                p.push(0, i, signal(i));
            }
            let sum = |c: usize| (0..16).map(|b| p.sumsq[b * ENV_COMPONENTS + c]).sum::<u64>();
            [sum(1), sum(2)]
        };
        let a = 1000i16;
        let dc = band_energy(&|_| a);
        assert_eq!(dc, [0, 0], "DC leaked into a detail channel");
        let square = |half_period: usize| move |i: usize| if (i / half_period) % 2 == 0 { a } else { -a };
        let nyq = band_energy(&square(1));
        assert!(nyq[0] > 0 && nyq[1] == 0, "period 2 should be d1 only: {nyq:?}");
        let p4 = band_energy(&square(2));
        assert!(p4[1] > 0 && p4[0] == 0, "period 4 should be d2 only: {p4:?}");
        let p8 = band_energy(&square(4));
        assert_eq!(p8, [0, 0], "period 8 sits below both details: {p8:?}");
    }

    #[test]
    fn fills_land_by_seq_beside_the_windows_that_arrived_and_never_overwrite() {
        use crate::call::spool::{FILL_FLAG, RAW_FLAG};
        let ops = vsf::OSCILLATIONS_PER_SECOND as i64;
        let raw = |v: u8| vec![v; FRAME_IN * 2];
        // Remote channel, two-frame windows: seq 10 arrived (slots 0,1), seq 11 LOST, seq 12 arrived (slots 4,5); the fill for 11 comes late with a stamp far in the future.
        let mut records = vec![
            Record { chan: 1 | RAW_FLAG, osc: 0, seq: Some((10, 0)), proc: None, bytes: raw(1) },
            Record { chan: 1 | RAW_FLAG, osc: ops / 200, seq: Some((10, 1)), proc: None, bytes: raw(2) },
            Record { chan: 1 | RAW_FLAG, osc: 4 * ops / 200, seq: Some((12, 0)), proc: None, bytes: raw(5) },
            Record { chan: 1 | RAW_FLAG, osc: 5 * ops / 200, seq: Some((12, 1)), proc: None, bytes: raw(6) },
            Record { chan: 1 | RAW_FLAG | FILL_FLAG, osc: 99 * ops, seq: Some((11, 0)), proc: None, bytes: raw(3) },
            Record { chan: 1 | RAW_FLAG | FILL_FLAG, osc: 99 * ops, seq: Some((11, 1)), proc: None, bytes: raw(4) },
            // A stale fill for a window that DID arrive must not replace it.
            Record { chan: 1 | RAW_FLAG | FILL_FLAG, osc: 99 * ops, seq: Some((12, 0)), proc: None, bytes: raw(0xEE) },
        ];
        records.push(Record { chan: 0 | RAW_FLAG, osc: 0, seq: Some((0, 0)), proc: None, bytes: raw(9) });
        let (nchan, grid) = grid_from_records(&records).unwrap();
        assert_eq!(nchan, 2);
        let first = |c: &Option<Cell>| match c { Some(Cell::Pcm(b, _)) => b[0], _ => 0xFF };
        assert_eq!((0..6).map(|s| first(&grid[1][s])).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(grid[1].len(), 6);
    }

    #[test]
    fn raw_mic_records_outrank_the_wire_copy_for_their_channel() {
        use crate::call::spool::{PROC_FLAG, RAW_FLAG};
        let ops = vsf::OSCILLATIONS_PER_SECOND as i64;
        let raw = |v: u8| vec![v; FRAME_IN * 2];
        let records = vec![
            // Channel 0: the raw mic (PROC) and the ducked wire copy of the same frames — the grid must hold the raw ones.
            Record { chan: RAW_FLAG | PROC_FLAG, osc: 0, seq: Some((0, 0)), proc: Some((128, 1)), bytes: raw(7) },
            Record { chan: RAW_FLAG, osc: 0, seq: Some((0, 0)), proc: None, bytes: raw(1) },
            Record { chan: RAW_FLAG | PROC_FLAG, osc: ops / 200, seq: Some((0, 1)), proc: Some((256, 0)), bytes: raw(8) },
            Record { chan: RAW_FLAG, osc: ops / 200, seq: Some((0, 1)), proc: None, bytes: raw(2) },
            // Channel 1 (remote) has only wire cells, as ever.
            Record { chan: 1 | RAW_FLAG, osc: 0, seq: Some((5, 0)), proc: None, bytes: raw(3) },
        ];
        let (nchan, grid) = grid_from_records(&records).unwrap();
        assert_eq!(nchan, 2);
        let first = |c: &Option<Cell>| match c { Some(Cell::Pcm(b, _)) => b[0], _ => 0xFF };
        assert_eq!((first(&grid[0][0]), first(&grid[0][1])), (7, 8));
        // The raw-mic cell carries the live gain (128/256 = half) and decodes with it applied.
        let mut dec = mono_decoder().unwrap();
        let pcm = decode_slot_n(&mut dec, &grid[0][0], FRAME_IN);
        let raw7 = i16::from_le_bytes([7, 7]);
        assert_eq!(pcm[0], (raw7 as f32 * 0.5) as i16);
        assert_eq!(first(&grid[1][0]), 3);
    }
}
