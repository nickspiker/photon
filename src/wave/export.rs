//! EXPORT a kept wave as a file other programs open (Nick 2026-09-28: "WAV and VSF — when they choose export it asks which flavour; might even add formats later").
//!
//! The kept blob (`PHWAVE9`) is Photon's own container: verbatim Opus packets per channel, holes as empty packets. Nothing outside Photon plays it, so an export always DECODES it — the same decode playback runs — into a stream of 48 kHz frames, channel 0 the recorder's own voice, the rest the other party, holes as silence.
//! WAV: 16-bit PCM, one channel per party, streamed to disk frame by frame (bounded memory, whatever the length).
//! VSF: the recorded Opus packets themselves, NOT decoded (Nick 2026-09-28: "no need to burn 10x the space") — one section per channel named by role (`self`, `peer`), each packet its own `v('o', …)` value (holes as empty ones) beside the channel's packet length, and a `wave` section with the rate and the recording's start in Eagle time.

use super::record;

/// The export flavours, in the order the chooser shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Wav,
    Vsf,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 2] = [ExportFormat::Wav, ExportFormat::Vsf];

    /// The pill label and file extension — format names, not words, so they read the same in every language.
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Wav => "WAV",
            ExportFormat::Vsf => "VSF",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            ExportFormat::Wav => "wav",
            ExportFormat::Vsf => "vsf",
        }
    }
}

const RATE: u32 = 48_000;

/// The recording's first-sample instant (Eagle oscillations) from the container header, `None` on a short or foreign blob.
fn base_osc(bytes: &[u8]) -> Option<i64> {
    // PHWAVE9: magic(8) ‖ nchan u8 ‖ rate u32 ‖ base i64 …
    let b = bytes.get(8 + 5..8 + 13)?;
    Some(i64::from_le_bytes(b.try_into().ok()?))
}

/// Decode `blob` (a kept `PHWAVE9`) into `out` in `fmt`. Returns the number of sample frames written.
/// This module never opens a file: the caller hands it the writer (the user-directed save lives in ui/photon_app/attachments.rs, the one sanctioned Downloads writer — scripts/lib/artifact-gate.sh).
pub fn export_to<W: std::io::Write + std::io::Seek>(blob: &[u8], fmt: ExportFormat, out: &mut W) -> Result<u64, String> {
    let mut stream = record::open_blob(blob).ok_or("not a kept wave (unknown container)")?;
    // A profiled wave (development builds, 2026-10-02) carries the far party AS PLAYED beside the wire channels: the WAV gets it as its last channel, the VSF as a "rendered" section.
    let rendered = record::open_render_profile(blob);
    match fmt {
        ExportFormat::Wav => write_wav(&mut stream, rendered, out),
        ExportFormat::Vsf => write_vsf(&mut stream, rendered, base_osc(blob), out),
    }
}

fn write_wav<W: std::io::Write + std::io::Seek>(stream: &mut record::KeptStream, rendered: Option<record::RenderProfile>, f: &mut W) -> Result<u64, String> {
    use std::io::{SeekFrom, Write};
    let wire = stream.nchan;
    let nchan = wire + usize::from(rendered.is_some());
    let mut rdec = rendered.as_ref().and_then(|_| opus::Decoder::new(48_000, opus::Channels::Mono).ok());
    let mut rslot = 0usize;
    let mut w = std::io::BufWriter::new(&mut *f);
    let block = (nchan * 2) as u16;
    // RIFF header with the two sizes patched once the length is known.
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&0u32.to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes()); // PCM
    h.extend_from_slice(&(nchan as u16).to_le_bytes());
    h.extend_from_slice(&RATE.to_le_bytes());
    h.extend_from_slice(&(RATE * block as u32).to_le_bytes());
    h.extend_from_slice(&block.to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&0u32.to_le_bytes());
    w.write_all(&h).map_err(|e| format!("write: {e}"))?;
    let mut frames: u64 = 0;
    let mut buf = Vec::new();
    let mut rpcm = vec![0i16; 480];
    while let Some(frame) = stream.next_frame() {
        buf.clear();
        let per = frame.len() / wire.max(1);
        if let (Some(rp), Some(dec)) = (rendered.as_ref(), rdec.as_mut()) {
            // The rendered slot in lockstep with the wire slot (both 10 ms); an empty packet or a slot past the profile is silence, the decoder untouched.
            rpcm.fill(0);
            if let Some(p) = rp.packets.get(rslot) {
                if !p.is_empty() {
                    let _ = dec.decode(p, &mut rpcm, false);
                }
            }
            rslot += 1;
            for i in 0..per {
                for ch in 0..wire {
                    buf.extend_from_slice(&frame[i * wire + ch].to_le_bytes());
                }
                buf.extend_from_slice(&rpcm.get(i).copied().unwrap_or(0).to_le_bytes());
            }
        } else {
            for s in &frame {
                buf.extend_from_slice(&s.to_le_bytes());
            }
        }
        w.write_all(&buf).map_err(|e| format!("write: {e}"))?;
        frames += per as u64;
    }
    let data = frames * block as u64;
    // WHY/PROOF: a RIFF size is 32 bits — past 4 GiB of samples (~6 h of stereo) the file cannot say its own length; refuse rather than write a lying header.
    let data32 = u32::try_from(data).map_err(|_| "longer than a WAV can hold (4 GiB)".to_string())?;
    w.into_inner().map_err(|e| format!("flush: {e}"))?; // flushes the buffer and hands the writer back for the header patch
    f.seek(SeekFrom::Start(4)).map_err(|e| format!("seek: {e}"))?;
    f.write_all(&(36 + data32).to_le_bytes()).map_err(|e| format!("write: {e}"))?;
    f.seek(SeekFrom::Start(40)).map_err(|e| format!("seek: {e}"))?;
    f.write_all(&data32.to_le_bytes()).map_err(|e| format!("write: {e}"))?;
    Ok(frames)
}

fn write_vsf<W: std::io::Write>(stream: &mut record::KeptStream, rendered: Option<record::RenderProfile>, start: Option<i64>, out: &mut W) -> Result<u64, String> {
    use vsf::VsfType;
    // COMPRESSED, not decoded (Nick 2026-09-28: "no need to burn 10x the space when it could just get reframed"): the kept Opus packets exactly as recorded, each one its own `v('o', …)` value, so a VSF reader needs no length-prefix parsing and any audio tool with Opus plays them.
    let (subs, chans) = stream.raw_packets().ok_or("no packets to export")?;
    let mut builder = vsf::VsfBuilder::new().creation_time_oscillations(vsf::eagle_time_oscillations()).provenance_only();
    let mut wave = vsf::VsfSection::new("wave");
    wave.add_field_multi("rate", vec![VsfType::u(RATE as usize, false)]);
    if let Some(s) = start {
        wave.add_field_multi("start", vec![VsfType::e(vsf::types::EtType::e6(s))]);
    }
    builder = builder.add_section_direct(wave);
    let mut slots = 0u64;
    for (c, packets) in chans.into_iter().enumerate() {
        let sub = subs.get(c).copied().unwrap_or(1).max(1) as u64;
        if c == 0 {
            slots = packets.len() as u64 / sub;
        }
        // One section per channel, named by ROLE — the recorder's own voice, then the other party — never a name or handle. Packet length declared so a hole (an empty packet) keeps its place in time.
        let mut sec = vsf::VsfSection::new(if c == 0 { "self" } else { "peer" });
        sec.add_field_multi("packet_ms", vec![VsfType::u((10 / sub) as usize, false)]);
        sec.add_field_multi("opus", packets.into_iter().map(|p| VsfType::v(b'o', p)).collect());
        builder = builder.add_section_direct(sec);
    }
    if let Some(rp) = rendered {
        // The instrument's channel: the far party as the DAC played it, 10 ms packets, with the composed render gain (8.8) and the hole count per slot.
        let mut sec = vsf::VsfSection::new("rendered");
        sec.add_field_multi("packet_ms", vec![VsfType::u(10, false)]);
        sec.add_field_multi("gain_q8", rp.slots.iter().map(|(g, _)| VsfType::u(*g as usize, false)).collect());
        sec.add_field_multi("holes", rp.slots.iter().map(|(_, h)| VsfType::u(*h as usize, false)).collect());
        sec.add_field_multi("opus", rp.packets.into_iter().map(|p| VsfType::v(b'o', p)).collect());
        builder = builder.add_section_direct(sec);
    }
    let bytes = builder.build().map_err(|e| format!("build: {e}"))?;
    out.write_all(&bytes).map_err(|e| format!("write: {e}"))?;
    Ok(slots * (RATE as u64 / 100))
}

/// The export's file name: `wave-<local date and time>-<who>.<ext>`, with anything a file system would choke on replaced.
pub fn file_name(start_osc: i64, who: &str, fmt: ExportFormat) -> String {
    let (secs, _) = vsf::types::to_unix_ns(start_osc);
    let when = chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d-%H%M").to_string())
        .unwrap_or_else(|| "undated".into());
    let who: String = who.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("wave-{when}-{who}.{}", fmt.ext())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kept two-party wave (the same build the keep runs) exported both ways: the WAV is a well-formed RIFF whose sizes match the samples written and whose audio is not silent; the VSF is a file that opens; a foreign blob is refused.
    #[test]
    fn exports_round_trip() {
        let mut enc = opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Audio).unwrap();
        let mut buf = vec![0u8; 4000];
        let ops = vsf::OSCILLATIONS_PER_SECOND as i64;
        let mut records = Vec::new();
        for i in 0..20i64 {
            let tone: Vec<i16> = (0..crate::platform::audio::FRAME_SAMPLES).map(|s| ((s as f32 * 0.1).sin() * 4000.0) as i16).collect();
            let n = enc.encode(&tone, &mut buf).unwrap();
            for chan in 0..2 {
                records.push(crate::wave::spool::Record { chan, osc: i * (ops / 200), seq: None, proc: None, bytes: buf[..n].to_vec() });
            }
        }
        let blob = record::build_container(&records).unwrap().container;
        let mut wav = std::io::Cursor::new(Vec::new());
        let frames = export_to(&blob, ExportFormat::Wav, &mut wav).unwrap();
        let bytes = wav.into_inner();
        assert!(frames > 0);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2, "one channel per party");
        let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        assert_eq!(data, frames as usize * 4);
        assert_eq!(bytes.len(), 44 + data);
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, 36 + data);
        assert!(bytes[44..].chunks_exact(2).any(|s| i16::from_le_bytes([s[0], s[1]]).unsigned_abs() > 100), "audible");
        let mut vsf_out = std::io::Cursor::new(Vec::new());
        assert_eq!(export_to(&blob, ExportFormat::Vsf, &mut vsf_out).unwrap(), frames);
        let v = vsf_out.into_inner();
        let vsf_len = v.len() as u64;
        assert!(vsf_len * 3 < bytes.len() as u64, "the Opus VSF is a fraction of the WAV: {vsf_len} vs {}", bytes.len());
        // Every recorded packet is in it verbatim.
        let (_, chans) = record::open_blob(&blob).unwrap().raw_packets().unwrap();
        for p in chans.iter().flatten().filter(|p| !p.is_empty()) {
            assert!(v.windows(p.len()).any(|w| w == &p[..]), "a packet is missing from the VSF");
        }
        assert!(export_to(b"not a wave at all, just bytes........", ExportFormat::Wav, &mut std::io::Cursor::new(Vec::new())).is_err());
    }

    #[test]
    fn names_are_file_safe() {
        let n = file_name(vsf::types::from_unix_ns(1_790_000_000, 0), "Emma / Bob?", ExportFormat::Wav);
        assert!(n.starts_with("wave-") && n.ends_with("-Emma___Bob_.wav"), "{n}");
    }
}
