//! Kept-recording transcode + reader (docs/calls.md — endpoint memory).
//!
//! The live call spools already-ENCODED per-direction mono Opus frames (cheap: ~25 MB/hour, `call/spool.rs`). At KEEP the user wants a real audio FILE with **one channel per participant** (ch0 = local mic, ch1 = remote), so this module transcodes the spool ONCE: decrypt → decode each direction → time-align onto a shared 10 ms grid by eagle-osc → interleave → re-encode as a single interleaved Opus, in the `PHCALL2` container. Playback ([`crate::call::playback`]) reads it back and sums the channels to mono.
//!
//! **N > 2 (future multi-party — the stubbed "add handle").** `opus` 0.3.1 has no multistream encoder, so a genuine ≥3-channel Opus is unreachable in this crate. The `nchan` header lets the container degrade gracefully: for `nchan ≤ 2` each 10 ms slot is ONE interleaved packet (mono or stereo — the true N-channel-Opus case); for `nchan > 2` each slot is `nchan` side-by-side MONO packets. Same magic, same reader, one downmix path. A true ≥3-channel Opus is a later opus-binding swap.
//!
//! **Container `PHCALL2\0`:** magic ‖ `[nchan u8][sample_rate u32 LE][base_osc i64 LE][slots u32 LE]` then `slots` records: for `nchan ≤ 2` one `[len u16 LE][opus]`; for `nchan > 2` exactly `nchan` such `[len u16 LE][opus]` back to back (one per channel, in channel order). Empty slots are encoded silence — the grid is dense so playback never has to reason about gaps.
//!
//! Transcode is a second lossy Opus generation over the spooled frames (decode-then-re-encode) — the accepted cost of "cheap live spool, rich keep". It is O(call length); run it OFF the UI thread (see `keep_recording`).

use crate::call::spool::{drain_records, SpoolTicket};

/// 10 ms at 48 kHz — the ARCHIVE frame (PHCALL2 slot + KeptStream playback unit). Deliberately unchanged by the 5ms flag day: every previously-kept recording plays without migration.
const FRAME: usize = 480;
/// The LIVE spool frame — 5ms CELT packets since the 2026-09-08 flag day (platform FRAME_SAMPLES). Two input slots fold into one archive slot at transcode.
const FRAME_IN: usize = crate::platform::audio::FRAME_SAMPLES;
/// Input-grid slots per second (5 ms spool packets).
const SLOTS_PER_SEC: i64 = 200;
/// PHCALL4 (flag day 2026-09-09, the coloured wave card): `PHCALL2` header ‖ `[env_per_sec u8][env_len u32 LE]` ‖ `nchan × env_len × ENV_COMPONENTS` envelope bytes (channel-major, bucket-major, [amp, r, g, b] each in eighth-stops below full scale, 255 = silence floor) ‖ packets. The envelope is computed at transcode — every frame is decoded here anyway — so no draw path ever decodes audio to show a shape. No PHCALL2 reader: nobody has waved for real yet.
pub const CONTAINER_MAGIC_V4: &[u8; 8] = b"PHCALL4\0";
/// Envelope components per bucket per channel: [amplitude, red, green, blue] — amplitude is the plain RMS; the colour bands are three HIGH-PASS energies at three scales (Nick 2026-09-09: "1:1 filter for blue, 1:4 for green, 1:8 for red, all high pass"): blue = first difference (x[n] − x[n−1]), green = the difference of successive 4-sample sums ÷ 4, red = the difference of successive 8-sample sums ÷ 8. Running sums, no FFT. All four in eighth-stops below full scale.
pub const ENV_COMPONENTS: usize = 4;
/// The envelope's fixed length per channel (Nick 2026-09-10: "linear on the waveform, downsample it to 65536 samples, then bilinear down to preview width"): every recording carries exactly this many buckets per channel, whatever its length — a 10s wave upsamples its 10ms slots into them, a 2h wave folds ~7 slots into each. 65536 × 4 components × 2 channels = 512KB in the container.
pub const ENV_BUCKETS: usize = 65536;

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
    if bytes.len() < 8 + 22 || &bytes[..8] != CONTAINER_MAGIC_V4 {
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
/// Fine-envelope buckets per second of recording (250 ms — syllable rate; two hours = 28.8k bytes per channel).
/// Header byte kept for the format's shape: 0 = the fixed ENV_BUCKETS grid (every recording, since 2026-09-10); the old per-second cadence is gone.
pub const ENV_PER_SEC: usize = 0;
/// Envelope value for a bucket's RMS: eighth-stops below full scale, saturating at the silence floor (255). One stop = ×2 amplitude, so the scale is perceptual by construction (Nick: stops, never dB).
fn stops_u8(sumsq: f64, n: usize) -> u8 {
    if n == 0 {
        return 255;
    }
    let rms = (sumsq / n as f64).sqrt();
    if rms < 1.0 {
        return 255;
    }
    ((32768.0 / rms).log2() * 8.0).round().clamp(0.0, 255.0) as u8
}
/// Fold a fine envelope down to the row thumbnail: one gross of buckets per channel, each the LOUDEST (minimum stops) fine bucket in its span so peaks survive.
pub fn thumbnail(lin: &[Vec<f32>], nchan: usize) -> Vec<u8> {
    // `lin[ch * ENV_COMPONENTS + comp]` = per-slot LINEAR values; the thumbnail is the same fold to one gross of buckets, then stops.
    let nb = crate::types::WAVE_THUMB_BUCKETS;
    let k = ENV_COMPONENTS;
    let mut out = vec![255u8; nchan * nb * k];
    for ch in 0..nchan {
        for c in 0..k {
            let folded = resample_linear(&lin[ch * k + c], nb);
            for b in 0..nb {
                out[(ch * nb + b) * k + c] = lin_to_stops_u8(folded[b]);
            }
        }
    }
    out
}

/// Per-channel running state for the three high-pass bands: a 16-sample ring and the four running sums the band differences are made of.
struct BandState {
    hist: [i32; 16],
    pos: usize,
    s4: i64,
    s4p: i64,
    s8: i64,
    s8p: i64,
}

impl BandState {
    fn new() -> Self {
        BandState { hist: [0; 16], pos: 0, s4: 0, s4p: 0, s8: 0, s8p: 0 }
    }
    /// Push one sample; return (d1, d4, d8) — the three high-pass outputs at this sample.
    #[inline]
    fn push(&mut self, x: i32) -> (i64, i64, i64) {
        let back = |s: &Self, k: usize| s.hist[(s.pos + 16 - k) % 16] as i64;
        let x1 = back(self, 1);
        let x4 = back(self, 4);
        let x8 = back(self, 8);
        let x16 = back(self, 16);
        let xi = x as i64;
        self.s4 += xi - x4;
        self.s4p += x4 - x8;
        self.s8 += xi - x8;
        self.s8p += x8 - x16;
        self.hist[self.pos] = x;
        self.pos = (self.pos + 1) % 16;
        (xi - x1, (self.s4 - self.s4p) / 4, (self.s8 - self.s8p) / 8)
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
fn grid_from_records(records: &[(u8, i64, Vec<u8>)]) -> Option<(usize, Vec<Vec<Option<Vec<u8>>>>)> {
    if records.is_empty() {
        return None;
    }
    let base = records.iter().map(|(_, osc, _)| *osc).min()?;
    let nchan = (records.iter().map(|(c, _, _)| *c).max()? as usize) + 1;
    // LATTICE SLOTTING (field 2026-09-08, "super garbled" preview): frames are stamped at DRAIN, in 1ms engine-loop bursts — adjacent 5ms frames carry near-identical stamps, and slotting each by its own stamp collided them ("collision keeps the last" ate half the audio). Per channel the spool IS contiguous (appended in codec order), so slots advance on a LATTICE from the last anchor, and the stamp only re-anchors when it deviates past REANCHOR (a real gap: lost windows, an engine stall) — the learner's stamp-regularizer law, applied to the recording grid.
    let ops = vsf::OSCILLATIONS_PER_SECOND as i64;
    let reanchor = ops / 20; // 50ms — ten slots; burst jitter is ±ms, real gaps are bigger
    let mut lat: Vec<Option<(i64, i64)>> = vec![None; nchan]; // per channel: (next_slot, expected_osc)
    let mut slotted: Vec<(usize, usize, &Vec<u8>)> = Vec::with_capacity(records.len());
    let mut max_slot = 0usize;
    for (chan, osc, opus) in records {
        let c = *chan as usize;
        let slot = match lat[c] {
            Some((next, expected)) if (osc - expected).abs() < reanchor => next,
            _ => osc_to_slot(*osc, base),
        };
        let slot_u = slot.max(0) as usize;
        lat[c] = Some((slot + 1, base + (slot + 1) * ops / SLOTS_PER_SEC));
        max_slot = max_slot.max(slot_u);
        slotted.push((c, slot_u, opus));
    }
    let mut grid: Vec<Vec<Option<Vec<u8>>>> = vec![vec![None; max_slot + 1]; nchan];
    for (c, slot, opus) in slotted {
        grid[c][slot] = Some(opus.clone());
    }
    Some((nchan, grid))
}

fn mono_decoder() -> Option<opus::Decoder> {
    opus::Decoder::new(48_000, opus::Channels::Mono).ok()
}

/// Decode one channel's slot (or silence) with its running decoder, at the caller's frame size — `FRAME_IN` for live-spool input (transcode + preview), `FRAME` for PHCALL2 archive playback. Silence slots do NOT touch the decoder (no frame was ever encoded there — the gap is genuine), so the next real frame decodes in step.
fn decode_slot_n(dec: &mut opus::Decoder, cell: &Option<Vec<u8>>, frame: usize) -> Vec<i16> {
    match cell {
        Some(opus) => {
            let mut pcm = vec![0i16; frame];
            match dec.decode(opus, &mut pcm, false) {
                Ok(n) if n == frame => pcm,
                _ => vec![0i16; frame],
            }
        }
        None => vec![0i16; frame],
    }
}

/// KEEP with transcode → a true N-channel (stereo for 1:1) Opus in the `PHCALL2` container, stored as a content-addressed blob. Returns (content_hash, size); consumes the ticket (dropping it crypto-shreds the spool key either way); removes the spool file on success. `None` = nothing recorded (treat keep as delete) or a codec init failure.
pub fn finalize_nchannel(ticket: SpoolTicket, identity_seed: &[u8; 32]) -> Option<Kept> {
    let records = drain_records(&ticket)?;
    let Some(t) = build_container(&records) else {
        crate::call::spool::shred(ticket);
        return None;
    };
    let hash = *blake3::hash(&t.container).as_bytes();
    let size = t.container.len() as u64;
    crate::storage::blob_store(identity_seed, &hash, &t.container).ok()?;
    let _ = std::fs::remove_file(&ticket.path);
    Some(Kept { hash, size, thumb: t.thumb, secs: t.secs })
}

/// The transcode core: drained spool records → a `PHCALL2` container (bytes). Split from [`finalize_nchannel`] so it's testable without the storage/vault layer. `None` = nothing recorded or a codec init failure.
pub(crate) fn build_container(records: &[(u8, i64, Vec<u8>)]) -> Option<Transcoded> {
    let (nchan, grid) = grid_from_records(records)?;
    let slots = grid[0].len();
    let slots_out = slots.div_ceil(2);
    let base = records.iter().map(|(_, osc, _)| *osc).min().unwrap_or(0);
    // Envelope accumulators: per channel, per 10ms SLOT, per component — sum of squares + sample count; folded to the fixed ENV_BUCKETS grid (and the row thumbnail) once the packets are written. The band states carry across slots so the filters see a continuous signal.
    let k = ENV_COMPONENTS;
    let mut sumsq = vec![0f64; nchan * slots_out * k];
    let mut counts = vec![0usize; nchan * slots_out];
    let mut bands: Vec<BandState> = (0..nchan).map(|_| BandState::new()).collect();
    let mut accumulate = |ch: usize, slot_out: usize, pcm: &[i16]| {
        let b = ch * slots_out + slot_out;
        let base = b * k;
        for &s in pcm {
            let x = s as i32;
            let (d1, d4, d8) = bands[ch].push(x);
            sumsq[base] += (x as f64) * (x as f64);
            sumsq[base + 1] += (d8 as f64) * (d8 as f64);
            sumsq[base + 2] += (d4 as f64) * (d4 as f64);
            sumsq[base + 3] += (d1 as f64) * (d1 as f64);
        }
        counts[b] += pcm.len();
    };

    // Packets first, header after: the header carries the envelope, which the packet loop produces.
    let mut container = Vec::with_capacity(slots * 48);

    let mut decs: Vec<opus::Decoder> = (0..nchan).map(|_| mono_decoder()).collect::<Option<_>>()?;
    let mut pkt = vec![0u8; 4000];
    let write_pkt = |container: &mut Vec<u8>, enc: &[u8]| {
        container.extend_from_slice(&(enc.len() as u16).to_le_bytes());
        container.extend_from_slice(enc);
    };

    if nchan <= 2 {
        // True N-channel Opus: one interleaved packet per slot. Application::Audio (archival — quality over the call's low-latency floor), VBR on.
        let chans = if nchan == 2 {
            opus::Channels::Stereo
        } else {
            opus::Channels::Mono
        };
        let mut enc = opus::Encoder::new(48_000, chans, opus::Application::Audio).ok()?;
        let _ = enc.set_vbr(true);
        let _ = enc.set_bitrate(opus::Bitrate::Bits(if nchan == 2 { 96_000 } else { 48_000 }));
        for slot_out in 0..slots_out {
            // Two 5ms input slots fold into one 10ms archive slot — the archive format (and every old kept blob) stays 10ms.
            let mut interleaved = vec![0i16; FRAME * nchan];
            for ch in 0..nchan {
                for half in 0..2 {
                    let slot_in = slot_out * 2 + half;
                    let pcm = if slot_in < slots {
                        decode_slot_n(&mut decs[ch], &grid[ch][slot_in], FRAME_IN)
                    } else {
                        vec![0i16; FRAME_IN]
                    };
                    accumulate(ch, slot_out, &pcm);
                    for (i, &s) in pcm.iter().enumerate() {
                        interleaved[(half * FRAME_IN + i) * nchan + ch] = s;
                    }
                }
            }
            let n = enc.encode(&interleaved, &mut pkt).ok()?;
            write_pkt(&mut container, &pkt[..n]);
        }
    } else {
        // N > 2 fallback: nchan side-by-side MONO packets per slot (no multistream Opus in this crate).
        let mut encs: Vec<opus::Encoder> = (0..nchan)
            .map(|_| {
                let mut e = opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Audio).ok()?;
                let _ = e.set_vbr(true);
                let _ = e.set_bitrate(opus::Bitrate::Bits(48_000));
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
                accumulate(ch, slot_out, &pcm);
                let n = encs[ch].encode(&pcm, &mut pkt).ok()?;
                write_pkt(&mut container, &pkt[..n]);
            }
        }
    }

    if container.is_empty() {
        return None;
    }
    // Per-slot LINEAR RMS per channel × component, then the fixed-grid fold (65536 per channel) and the row thumbnail from the same linear values.
    let lin: Vec<Vec<f32>> = (0..nchan * k)
        .map(|ci| {
            let (ch, c) = (ci / k, ci % k);
            (0..slots_out)
                .map(|slot| {
                    let b = ch * slots_out + slot;
                    if counts[b] == 0 { 0.0 } else { (sumsq[b * k + c] / counts[b] as f64).sqrt() as f32 }
                })
                .collect()
        })
        .collect();
    let env_len = ENV_BUCKETS;
    let mut fine = vec![255u8; nchan * env_len * k];
    for ch in 0..nchan {
        for c in 0..k {
            let folded = resample_linear(&lin[ch * k + c], env_len);
            for b in 0..env_len {
                fine[(ch * env_len + b) * k + c] = lin_to_stops_u8(folded[b]);
            }
        }
    }
    let thumb = thumbnail(&lin, nchan);
    let mut out = Vec::with_capacity(CONTAINER_MAGIC_V4.len() + 22 + fine.len() + container.len());
    out.extend_from_slice(CONTAINER_MAGIC_V4);
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
    /// Total playable frames (archive slots for PHCALL3, grid slots for a live-spool preview) — the scrub bar's denominator.
    pub total: usize,
    /// The fine envelope from the container header (`nchan × env_len × ENV_COMPONENTS`, eighth-stops) — empty for a live-spool preview.
    pub envelope: Vec<u8>,
    pub env_per_sec: u8,
    inner: Inner,
}

enum Inner {
    /// PHCALL2, nchan ≤ 2: one sequential decoder, each packet already interleaved.
    Packed {
        bytes: Vec<u8>,
        cur: usize,
        dec: opus::Decoder,
    },
    /// PHCALL2, nchan > 2: nchan mono decoders, `nchan` packets per slot.
    Multi {
        bytes: Vec<u8>,
        cur: usize,
        decs: Vec<opus::Decoder>,
    },
    /// Live-spool grid (the Ended-screen PREVIEW path): 5ms input slots, slot-iterated + interleaved on the fly.
    Grid {
        grid: Vec<Vec<Option<Vec<u8>>>>,
        decs: Vec<opus::Decoder>,
        slot: usize,
    },
}

/// Open a kept-call blob for playback — `PHCALL2` only (PHCALL1 read support deleted with the 5ms flag day). `None` on unknown magic or codec init failure.
pub fn open_blob(bytes: &[u8]) -> Option<KeptStream> {
    if bytes.len() >= 8 && &bytes[..8] == CONTAINER_MAGIC_V4 {
        if bytes.len() < 8 + 22 {
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
        let inner = if nchan <= 2 {
            let chans = if nchan == 2 {
                opus::Channels::Stereo
            } else {
                opus::Channels::Mono
            };
            Inner::Packed {
                bytes: body,
                cur: 0,
                dec: opus::Decoder::new(48_000, chans).ok()?,
            }
        } else {
            Inner::Multi {
                bytes: body,
                cur: 0,
                decs: (0..nchan).map(|_| mono_decoder()).collect::<Option<_>>()?,
            }
        };
        Some(KeptStream { nchan, total, envelope, env_per_sec, inner })
    } else {
        // PHCALL1/2/3 read support deleted with their flag days (nobody waving yet, no backwards compat) — unknown magic is unknown magic.
        None
    }
}

/// Build a playable stream directly from drained spool records — the Ended-screen PREVIEW path, so Play works before Keep finalizes a blob.
pub(crate) fn stream_from_records(records: &[(u8, i64, Vec<u8>)]) -> Option<KeptStream> {
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
            Inner::Packed { bytes, cur, dec } => {
                *cur = 0;
                for _ in 0..start {
                    if read_pkt(bytes, cur).is_none() {
                        return;
                    }
                }
                let _ = dec.reset_state();
            }
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
            Inner::Packed { bytes, cur, dec } => {
                let opus = read_pkt(bytes, cur)?;
                let mut out = vec![0i16; FRAME * nchan];
                let _ = dec.decode(opus, &mut out, false);
                Some(out)
            }
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
        // Build two directions of encoded frames, transcode to a PHCALL2 container, reopen, assert stereo + audible. No storage/vault (build_container is the transcode core).
        let mut enc =
            opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Audio).unwrap();
        let mut buf = vec![0u8; 4000];
        let ops = vsf::OSCILLATIONS_PER_SECOND as i64;
        let mut records: Vec<(u8, i64, Vec<u8>)> = Vec::new();
        for i in 0..20i64 {
            // A tone so decode is non-zero; both directions, 5 ms apart (the live spool cadence) — 20 input slots fold to 10 archive slots.
            let tone: Vec<i16> = (0..FRAME_IN)
                .map(|s| ((s as f32 * 0.1).sin() * 4000.0) as i16)
                .collect();
            let n = enc.encode(&tone, &mut buf).unwrap();
            let osc = i * (ops / 200);
            records.push((0, osc, buf[..n].to_vec()));
            records.push((1, osc, buf[..n].to_vec()));
        }
        let t = build_container(&records).unwrap();
        let container = t.container;
        assert_eq!(&container[..8], CONTAINER_MAGIC_V4);
        // The row thumbnail is one gross of buckets per channel, four components each, and a tone is well above the silence floor in every bucket that has audio.
        assert_eq!(t.thumb.len(), 2 * crate::types::WAVE_THUMB_BUCKETS * ENV_COMPONENTS);
        assert!(t.thumb.iter().any(|&b| b < 255), "thumbnail shows only silence");
        let mut ks = open_blob(&container).unwrap();
        assert_eq!(ks.nchan, 2);
        assert_eq!(ks.env_per_sec as usize, ENV_PER_SEC);
        assert_eq!(ks.envelope.len(), 2 * ENV_BUCKETS * ENV_COMPONENTS);
        assert!(ks.envelope.iter().any(|&b| b < 255), "fine envelope shows only silence");
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
}
