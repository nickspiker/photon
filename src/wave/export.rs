//! EXPORT a kept wave as a file other programs open (Nick 2026-09-28: "WAV and VSF — when they choose export it asks which flavour; might even add formats later").
//!
//! The kept blob (`PHWAVE9`) is Photon's own container: verbatim Opus packets per channel, holes as empty packets. Nothing outside Photon plays it, so an export always DECODES it — the same decode playback runs — into a stream of 48 kHz frames, channel 0 the recorder's own voice, the rest the other party, holes as silence.
//! WAV: 16-bit PCM, one channel per party, streamed to disk frame by frame (bounded memory, whatever the length).
//! VSF: the same samples as a typed i16 tensor `[channels × samples]` with the rate, the recording's start in Eagle time and each channel's role — exact samples, no codec, readable by any VSF reader.

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

/// Decode `blob` (a kept `PHWAVE9`) and write it to `path` in `fmt`. Returns the number of sample frames written.
pub fn export_to(blob: &[u8], fmt: ExportFormat, path: &std::path::Path) -> Result<u64, String> {
    let mut stream = record::open_blob(blob).ok_or("not a kept wave (unknown container)")?;
    let nchan = stream.nchan;
    match fmt {
        ExportFormat::Wav => write_wav(&mut stream, nchan, path),
        ExportFormat::Vsf => write_vsf(&mut stream, nchan, base_osc(blob), path),
    }
}

fn write_wav(stream: &mut record::KeptStream, nchan: usize, path: &std::path::Path) -> Result<u64, String> {
    use std::io::{Seek, SeekFrom, Write};
    let f = std::fs::File::create(path).map_err(|e| format!("create: {e}"))?;
    let mut w = std::io::BufWriter::new(f);
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
    while let Some(frame) = stream.next_frame() {
        buf.clear();
        for s in &frame {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        w.write_all(&buf).map_err(|e| format!("write: {e}"))?;
        frames += (frame.len() / nchan.max(1)) as u64;
    }
    let data = frames * block as u64;
    // WHY/PROOF: a RIFF size is 32 bits — past 4 GiB of samples (~6 h of stereo) the file cannot say its own length; refuse rather than write a lying header.
    let data32 = u32::try_from(data).map_err(|_| "longer than a WAV can hold (4 GiB)".to_string())?;
    let mut f = w.into_inner().map_err(|e| format!("flush: {e}"))?;
    f.seek(SeekFrom::Start(4)).map_err(|e| format!("seek: {e}"))?;
    f.write_all(&(36 + data32).to_le_bytes()).map_err(|e| format!("write: {e}"))?;
    f.seek(SeekFrom::Start(40)).map_err(|e| format!("seek: {e}"))?;
    f.write_all(&data32.to_le_bytes()).map_err(|e| format!("write: {e}"))?;
    Ok(frames)
}

fn write_vsf(stream: &mut record::KeptStream, nchan: usize, start: Option<i64>, path: &std::path::Path) -> Result<u64, String> {
    use vsf::VsfType;
    // Planar [channel][sample]: each party's voice is one contiguous row.
    let mut planes: Vec<Vec<i16>> = vec![Vec::new(); nchan];
    while let Some(frame) = stream.next_frame() {
        for (i, s) in frame.iter().enumerate() {
            planes[i % nchan].push(*s);
        }
    }
    let samples = planes.first().map_or(0, |p| p.len());
    let data: Vec<i16> = planes.into_iter().flatten().collect();
    let mut section = vsf::VsfSection::new("wave");
    section.add_field_multi("rate", vec![VsfType::u(RATE as usize, false)]);
    if let Some(s) = start {
        section.add_field_multi("start", vec![VsfType::e(vsf::types::EtType::e6(s))]);
    }
    // Channel 0 is the recorder's own voice; every other channel is the other party. Roles, never names or handles.
    let roles: Vec<VsfType> = (0..nchan).map(|c| VsfType::x(if c == 0 { "self".into() } else { "peer".into() })).collect();
    section.add_field_multi("channels", roles);
    section.add_field_multi("pcm", vec![VsfType::t_i4(vsf::types::Tensor { shape: vec![nchan, samples], data })]);
    let bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("build: {e}"))?;
    std::fs::write(path, &bytes).map_err(|e| format!("write: {e}"))?;
    Ok(samples as u64)
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
        let dir = std::env::temp_dir().join(format!("photon-export-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let wav = dir.join("w.wav");
        let frames = export_to(&blob, ExportFormat::Wav, &wav).unwrap();
        let bytes = std::fs::read(&wav).unwrap();
        assert!(frames > 0);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2, "one channel per party");
        let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        assert_eq!(data, frames as usize * 4);
        assert_eq!(bytes.len(), 44 + data);
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, 36 + data);
        assert!(bytes[44..].chunks_exact(2).any(|s| i16::from_le_bytes([s[0], s[1]]).unsigned_abs() > 100), "audible");
        let vsf_path = dir.join("w.vsf");
        assert_eq!(export_to(&blob, ExportFormat::Vsf, &vsf_path).unwrap(), frames);
        assert!(std::fs::metadata(&vsf_path).unwrap().len() as u64 > frames * 4);
        assert!(export_to(b"not a wave at all, just bytes........", ExportFormat::Wav, &dir.join("x.wav")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_are_file_safe() {
        let n = file_name(vsf::types::from_unix_ns(1_790_000_000, 0), "Emma / Bob?", ExportFormat::Wav);
        assert!(n.starts_with("wave-") && n.ends_with("-Emma___Bob_.wav"), "{n}");
    }
}
