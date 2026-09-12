//! Music decode for PIGEONS CARRYING WAVES (Nick 2026-09-12): a dropped song's row renders the same three-pyramid waveform a wave card does.
//! Symphonia decodes the pigeon's bytes in RAM (no temp file — it reads straight off the blob) and the PCM runs thru the wave pipeline's own accumulator, so the band in the row IS the wave visual, fed by mp3/flac/ogg/m4a/wav instead of a call.

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

/// Decode a music blob to interleaved i16 + (channels, rate). `None` for anything symphonia can't read — the row just keeps its bubble.
fn decode(bytes: &[u8]) -> Option<(Vec<i16>, usize, u32)> {
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
    let mut pcm: Vec<i16> = Vec::new();
    let mut nchan = 0usize;
    let mut rate = 48_000u32;
    let mut sbuf: Option<SampleBuffer<i16>> = None;
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        let Ok(audio) = decoder.decode(&packet) else {
            continue;
        };
        let spec = *audio.spec();
        nchan = spec.channels.count();
        rate = spec.rate;
        let cap = audio.capacity() as u64;
        let sb = sbuf.get_or_insert_with(|| SampleBuffer::<i16>::new(cap, spec));
        if sb.capacity() < audio.frames() * nchan {
            *sb = SampleBuffer::<i16>::new(cap, spec);
        }
        sb.copy_interleaved_ref(audio);
        pcm.extend_from_slice(sb.samples());
    }
    (!pcm.is_empty() && nchan > 0).then_some((pcm, nchan, rate))
}

/// A music pigeon's per-channel envelopes — the exact tensors a wave exchanges, off the UI thread.
pub fn envelopes_from_music(bytes: &[u8]) -> Option<Vec<crate::call::wave_env::WaveEnv>> {
    let (pcm, nchan, rate) = decode(bytes)?;
    let envs = crate::call::record::envelopes_from_pcm(&pcm, nchan, rate);
    (!envs.is_empty()).then_some(envs)
}
