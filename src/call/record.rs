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
/// Envelope components per bucket per channel: [amplitude, red, green, blue] — amplitude is the plain RMS; the colour bands are the top three detail levels of the decimated binary sum tree (Nick 2026-09-11: "binary trees"): blue = pair difference `x[n] − x[n−1]` (one value per 2 samples, highest octave), green = difference of adjacent pair-sums ÷ 2 (per 4 samples), red = difference of adjacent 4-sample sums ÷ 4 (per 8 samples). Adds and shifts, no FFT. All four in eighth-stops below full scale.
pub const ENV_COMPONENTS: usize = 4;
/// Envelope pyramid capacity (Nick 2026-09-11: "a bin that's 2^17 wide that triggers a resize to downsample all by 2"): the working buffer is this many bins per channel, and a recording stores whatever count it ends on (0..=ENV_CAP), so `env_len` is genuinely variable now.
pub const ENV_CAP: usize = 1 << 17;
/// Starting samples-per-bin exponent: 2^10 = 1024 samples = 21.3 ms — at least one full pitch period down to ~50 Hz, so every bin is a true envelope value from birth (sub-period bins were the chunking). The one tunable.
const ENV_S0_LOG2: u32 = 10;

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

/// Linear RMS (0..32768) → eighth-stops below full scale, the envelope byte.
fn lin_to_stops_u8(rms: f32) -> u8 {
    if rms < 1.0 {
        return 255;
    }
    ((32768.0 / rms).log2() * 8.0).round().clamp(0.0, 255.0) as u8
}

/// The envelope straight from a container's header — no audio decoded, no decoder built: (nchan, env_len, bytes). The wave card's loader.
pub fn envelope_of_blob(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if bytes.len() < 8 + 22 || &bytes[..8] != CONTAINER_MAGIC_V6 {
        return None;
    }
    let nchan = bytes[8] as usize;
    let env_len = u32::from_le_bytes(bytes[8 + 18..8 + 22].try_into().ok()?) as usize;
    let env_end = 8 + 22 + nchan * env_len * ENV_COMPONENTS;
    if nchan == 0 || bytes.len() < env_end {
        return None;
    }
    Some((nchan, env_len, bytes[8 + 22..env_end].to_vec()))
}
/// Header byte kept for the format's shape: 0 = the pyramid grid (variable `env_len`, every recording since 2026-09-11); the old per-second cadence is gone.
pub const ENV_PER_SEC: usize = 0;
/// Fold a fine envelope down to the row thumbnail: one gross of buckets per channel, folded in the POWER domain (square → box mean → root) so each thumb bucket is the true RMS over its span, then stops.
pub fn thumbnail(lin: &[Vec<f32>], nchan: usize) -> Vec<u8> {
    let nb = crate::types::WAVE_THUMB_BUCKETS;
    let k = ENV_COMPONENTS;
    let mut out = vec![255u8; nchan * nb * k];
    for ch in 0..nchan {
        for c in 0..k {
            let pow: Vec<f32> = lin[ch * k + c].iter().map(|v| v * v).collect();
            let folded = resample_linear(&pow, nb);
            for b in 0..nb {
                out[(ch * nb + b) * k + c] = lin_to_stops_u8(folded[b].max(0.0).sqrt());
            }
        }
    }
    out
}

/// Per-channel state for the decimated binary sum tree: the top three detail levels are the colour bands (Nick 2026-09-11: blue = pair difference, green = difference of adjacent pair-sums, red = the same fold on 4-sample sums; binary trees).
/// Each level stashes its left sibling and emits on the right one, so blue yields one value per 2 samples, green per 4, red per 8 — non-overlapping, exact, and every band lands on the same ≤2^16 amplitude scale via the mean-difference shifts.
struct HaarState {
    s1: i64,
    p2: i64,
    p4: i64,
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
            haar: (0..nchan).map(|_| HaarState { s1: 0, p2: 0, p4: 0 }).collect(),
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
        let h = &mut self.haar[ch];
        if abs_idx & 1 == 0 {
            h.s1 = xi;
            return;
        }
        let blue = xi - h.s1;
        let pair = xi + h.s1;
        self.sumsq[base + 3] += (blue * blue) as u64;
        self.counts[base + 3] += 1;
        if (abs_idx >> 1) & 1 == 0 {
            h.p2 = pair;
            return;
        }
        let green = (pair - h.p2) / 2;
        let quad = pair + h.p2;
        self.sumsq[base + 2] += (green * green) as u64;
        self.counts[base + 2] += 1;
        if (abs_idx >> 2) & 1 == 0 {
            h.p4 = quad;
            return;
        }
        let red = (quad - h.p4) / 4;
        self.sumsq[base + 1] += (red * red) as u64;
        self.counts[base + 1] += 1;
    }
    /// Reduce to (env_len, per-component linear RMS tracks): env_len = however many bins the recording ended on, 0..=ENV_CAP — stored as-is, the readers honour it.
    fn finish(&self) -> (usize, Vec<Vec<f32>>) {
        let k = ENV_COMPONENTS;
        let s = 1usize << self.s_log2;
        let env_len = self.total_samples.div_ceil(s).min(self.cap);
        let lin = (0..self.nchan * k)
            .map(|ci| {
                let (ch, c) = (ci / k, ci % k);
                (0..env_len)
                    .map(|bin| {
                        let i = (ch * self.cap + bin) * k + c;
                        if self.counts[i] == 0 { 0.0 } else { (self.sumsq[i] as f64 / self.counts[i] as f64).sqrt() as f32 }
                    })
                    .collect()
            })
            .collect();
        (env_len, lin)
    }
}

/// A finished transcode: the sealed container plus what the wave card needs without opening it.
pub struct Transcoded {
    pub container: Vec<u8>,
    /// Row thumbnail, `nchan × WAVE_THUMB_BUCKETS`.
    pub thumb: Vec<u8>,
    /// Whole seconds of audio (archive slots ÷ 100).
    pub secs: u32,
}

/// A kept blob as stored: content hash, byte size, and the card fields.
pub struct Kept {
    pub hash: [u8; 32],
    pub size: u64,
    pub thumb: Vec<u8>,
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
    let _ = std::fs::remove_file(&ticket.path);
    Some(Kept { hash, size, thumb: t.thumb, secs: t.secs })
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
    // Root-mean-square per bin per component — LINEAR, positive — straight into the envelope bytes; the row thumbnail folds the same linear bins to one gross.
    let k = ENV_COMPONENTS;
    let (env_len, lin) = pyramid.finish();
    let mut fine = vec![255u8; nchan * env_len * k];
    for ch in 0..nchan {
        for c in 0..k {
            for b in 0..env_len {
                fine[(ch * env_len + b) * k + c] = lin_to_stops_u8(lin[ch * k + c][b]);
            }
        }
    }
    let thumb = thumbnail(&lin, nchan);
    let mut out = Vec::with_capacity(CONTAINER_MAGIC_V6.len() + 22 + fine.len() + container.len());
    out.extend_from_slice(CONTAINER_MAGIC_V6);
    out.push(nchan as u8);
    out.extend_from_slice(&48_000u32.to_le_bytes());
    out.extend_from_slice(&base.to_le_bytes());
    out.extend_from_slice(&(slots_out as u32).to_le_bytes());
    out.push(ENV_PER_SEC as u8);
    out.extend_from_slice(&(env_len as u32).to_le_bytes());
    out.extend_from_slice(&fine);
    out.extend_from_slice(&container);
    Some(Transcoded { container: out, thumb, secs: (slots_out / 100) as u32 })
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
        let container = t.container;
        assert_eq!(&container[..8], CONTAINER_MAGIC_V6);
        // The row thumbnail is one gross of buckets per channel, four components each, and a tone is well above the silence floor in every bucket that has audio.
        assert_eq!(t.thumb.len(), 2 * crate::types::WAVE_THUMB_BUCKETS * ENV_COMPONENTS);
        assert!(t.thumb.iter().any(|&b| b < 255), "thumbnail shows only silence");
        let mut ks = open_blob(&container).unwrap();
        assert_eq!(ks.nchan, 2);
        assert_eq!(ks.env_per_sec as usize, ENV_PER_SEC);
        // Variable env_len: 10 archive slots = 4800 samples at 1024 samples a bin = 5 bins per channel.
        let expect_bins = (10 * FRAME).div_ceil(1usize << ENV_S0_LOG2);
        assert_eq!(ks.envelope.len(), 2 * expect_bins * ENV_COMPONENTS);
        assert!(ks.envelope.iter().any(|&b| b < 255), "fine envelope shows only silence");
        let (e_nchan, e_len, e_bytes) = envelope_of_blob(&container).unwrap();
        assert_eq!((e_nchan, e_len), (2, expect_bins));
        assert_eq!(e_bytes, ks.envelope);
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
        let (a_len, a_lin) = streamed.finish();
        let (b_len, b_lin) = direct.finish();
        assert_eq!(a_len, b_len);
        assert_eq!(a_lin, b_lin);
    }

    #[test]
    fn haar_bands_separate_octaves() {
        // The decimated tree's levels are octave-exclusive: DC excites nothing, a period-2 wave only blue, period-4 only green, period-8 only red.
        let band_energy = |signal: &dyn Fn(usize) -> i16| -> [u64; 3] {
            let mut p = EnvPyramid::with_geometry(1, 16, 10);
            for i in 0..4096 {
                p.push(0, i, signal(i));
            }
            let sum = |c: usize| (0..16).map(|b| p.sumsq[b * ENV_COMPONENTS + c]).sum::<u64>();
            [sum(1), sum(2), sum(3)]
        };
        let a = 1000i16;
        let dc = band_energy(&|_| a);
        assert_eq!(dc, [0, 0, 0], "DC leaked into a detail band");
        let nyq = band_energy(&|i| if i % 2 == 0 { a } else { -a });
        assert!(nyq[2] > 0 && nyq[0] == 0 && nyq[1] == 0, "period-2 should be blue only: {nyq:?}");
        let p4 = band_energy(&|i| if (i / 2) % 2 == 0 { a } else { -a });
        assert!(p4[1] > 0 && p4[0] == 0 && p4[2] == 0, "period-4 should be green only: {p4:?}");
        let p8 = band_energy(&|i| if (i / 4) % 2 == 0 { a } else { -a });
        assert!(p8[0] > 0 && p8[1] == 0 && p8[2] == 0, "period-8 should be red only: {p8:?}");
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
