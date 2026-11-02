//! Music decode for PIGEONS CARRYING WAVES (Nick 2026-09-12): a dropped song's row renders the same three-pyramid waveform a wave card does.
//! Symphonia decodes the pigeon's bytes in RAM (no temp file — it reads straight off the blob) and the PCM runs thru the wave pipeline's own accumulator, so the band in the row IS the wave visual, fed by mp3/flac/ogg/m4a/wav instead of a wave.

use symphonia::core::io::MediaSource;

/// The blob's bytes as a seekable media source.
struct Mem {
    cur: std::io::Cursor<Vec<u8>>,
}

impl std::io::Read for Mem {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.cur, buf)
    }
}

impl std::io::Seek for Mem {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.cur, pos)
    }
}

impl MediaSource for Mem {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.cur.get_ref().len() as u64)
    }
}

/// The longest stretch of a song drawn, in seconds: four and a half hours. A tiny compressed file can claim days of audio, and decoding all of it is CPU nobody asked for (security audit UPW-004); the band shows the first part.
const MAX_SECONDS_LOG2: u32 = 14;

/// A music pigeon's per-channel envelopes — the exact tensors a wave exchanges, off the UI thread. Symphonia decodes packet by packet straight into the envelope pyramid, so no song's PCM is ever whole in RAM (security audit UPW-004). `None` for anything symphonia can't read — the row just keeps its bubble.
pub fn envelopes_from_music(bytes: &[u8]) -> Option<Vec<crate::wave::wave_env::WaveEnv>> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let mss = MediaSourceStream::new(Box::new(Mem { cur: std::io::Cursor::new(bytes.to_vec()) }), Default::default());
    let probed = symphonia::default::get_probe().format(&Hint::new(), mss, &FormatOptions::default(), &MetadataOptions::default()).ok()?;
    let mut format = probed.format;
    let track = format.default_track()?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).ok()?;
    let mut env = crate::wave::record::PcmEnvelopes::default();
    let mut sbuf: Option<SampleBuffer<i16>> = None;
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        let Ok(audio) = decoder.decode(&packet) else {
            continue;
        };
        let spec = *audio.spec();
        let nchan = spec.channels.count();
        let cap = audio.capacity() as u64;
        let sb = sbuf.get_or_insert_with(|| SampleBuffer::<i16>::new(cap, spec));
        if sb.capacity() < audio.frames() * nchan {
            *sb = SampleBuffer::<i16>::new(cap, spec);
        }
        sb.copy_interleaved_ref(audio);
        let frames = env.push(sb.samples(), nchan, spec.rate);
        if frames as u64 >= (spec.rate as u64) << MAX_SECONDS_LOG2 {
            break;
        }
    }
    let envs = env.finish();
    (!envs.is_empty()).then_some(envs)
}
