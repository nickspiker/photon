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
pub const CONTAINER_MAGIC_V2: &[u8; 8] = b"PHCALL2\0";

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
pub fn finalize_nchannel(ticket: SpoolTicket, identity_seed: &[u8; 32]) -> Option<([u8; 32], u64)> {
    let records = drain_records(&ticket)?;
    let Some(container) = build_container(&records) else {
        crate::call::spool::shred(ticket);
        return None;
    };
    let hash = *blake3::hash(&container).as_bytes();
    let size = container.len() as u64;
    crate::storage::blob_store(identity_seed, &hash, &container).ok()?;
    let _ = std::fs::remove_file(&ticket.path);
    Some((hash, size))
}

/// The transcode core: drained spool records → a `PHCALL2` container (bytes). Split from [`finalize_nchannel`] so it's testable without the storage/vault layer. `None` = nothing recorded or a codec init failure.
pub(crate) fn build_container(records: &[(u8, i64, Vec<u8>)]) -> Option<Vec<u8>> {
    let (nchan, grid) = grid_from_records(records)?;
    let slots = grid[0].len();
    let slots_out = slots.div_ceil(2);
    let base = records.iter().map(|(_, osc, _)| *osc).min().unwrap_or(0);

    let mut container = Vec::with_capacity(CONTAINER_MAGIC_V2.len() + 17 + slots * 48);
    container.extend_from_slice(CONTAINER_MAGIC_V2);
    container.push(nchan as u8);
    container.extend_from_slice(&48_000u32.to_le_bytes());
    container.extend_from_slice(&base.to_le_bytes());
    container.extend_from_slice(&(slots_out as u32).to_le_bytes());

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
                let n = encs[ch].encode(&pcm, &mut pkt).ok()?;
                write_pkt(&mut container, &pkt[..n]);
            }
        }
    }

    if container.len() <= CONTAINER_MAGIC_V2.len() + 17 {
        return None;
    }
    Some(container)
}

/// A decoded recording as a stream of interleaved `FRAME × nchan` i16 frames. Bounded memory: the compressed container stays in RAM (~tens of MB/hour) and each 10 ms frame decodes on demand via [`Self::next_frame`] — never the whole PCM at once (a stereo hour is ~700 MB decoded).
pub struct KeptStream {
    pub nchan: usize,
    /// Total playable frames (archive slots for PHCALL2, grid slots for a live-spool preview) — the scrub bar's denominator.
    pub total: usize,
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
    if bytes.len() >= 8 && &bytes[..8] == CONTAINER_MAGIC_V2 {
        if bytes.len() < 8 + 17 {
            return None;
        }
        let nchan = bytes[8] as usize;
        // header: [nchan u8][rate u32][base i64][slots u32] = 1+4+8+4 = 17 bytes after magic; packets follow.
        let total = u32::from_le_bytes(bytes[8 + 13..8 + 17].try_into().ok()?) as usize;
        let body = bytes[8 + 17..].to_vec();
        if nchan == 0 {
            return None;
        }
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
        Some(KeptStream { nchan, total, inner })
    } else {
        // PHCALL1 read support deleted with the 5ms flag day (Nick 2026-09-08: nobody waving yet, no backwards compat) — unknown magic is unknown magic.
        None
    }
}

/// Build a playable stream directly from drained spool records — the Ended-screen PREVIEW path, so Play works before Keep finalizes a blob.
pub(crate) fn stream_from_records(records: &[(u8, i64, Vec<u8>)]) -> Option<KeptStream> {
    let (nchan, grid) = grid_from_records(records)?;
    Some(KeptStream {
        nchan,
        total: grid[0].len(),
        inner: Inner::Grid {
            grid,
            decs: (0..nchan).map(|_| mono_decoder()).collect::<Option<_>>()?,
            slot: 0,
        },
    })
}

impl KeptStream {
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
        let container = build_container(&records).unwrap();
        assert_eq!(&container[..8], CONTAINER_MAGIC_V2);
        let mut ks = open_blob(&container).unwrap();
        assert_eq!(ks.nchan, 2);
        let mut frames = 0;
        let mut energy = 0i64;
        while let Some(f) = ks.next_frame() {
            assert_eq!(f.len(), FRAME * 2);
            energy += f.iter().map(|&s| s.unsigned_abs() as i64).sum::<i64>();
            frames += 1;
        }
        assert!(frames >= 8, "expected ~10 slots, got {frames}");
        assert!(energy > 0, "decoded audio was pure silence");
    }
}
