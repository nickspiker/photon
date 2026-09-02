//! promptenc — one-shot dev tool: WAV master → raw opus packet stream for the embedded calibration prompts.
//! Usage: `promptenc <in.wav> <out.opusp>` (input: 48kHz s16 WAV, mono or stereo — stereo mixes to mono; the master is already normalized).
//! Output framing ("PHPRMPT1"): magic, then [len u16 LE][opus packet] repeating — the call spool's trivial length-prefix idiom, no container. Encoded exactly like the call's top rung (48k mono CELT LowDelay @ 128kbps, 10ms/480-sample frames) so the decode side is the same libopus the calls already vendor.

const FRAME_SAMPLES: usize = 480; // 10ms @ 48k — matches platform::audio::FRAME_SAMPLES

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(inp), Some(outp)) = (args.next(), args.next()) else {
        eprintln!("usage: promptenc <in.wav> <out.opusp>");
        std::process::exit(2);
    };
    let wav = std::fs::read(&inp).expect("read input wav");
    let mono = wav_to_mono_48k(&wav);
    eprintln!("promptenc: {} samples ({:.2}s) mono 48k", mono.len(), mono.len() as f64 / 48_000.0);

    let mut enc = opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::LowDelay)
        .expect("opus encoder");
    enc.set_bitrate(opus::Bitrate::Bits(128_000)).expect("bitrate");

    let mut out: Vec<u8> = b"PHPRMPT1".to_vec();
    let mut buf = vec![0u8; 4000];
    let mut packets = 0usize;
    for chunk in mono.chunks(FRAME_SAMPLES) {
        // Zero-pad the tail frame — 10ms of trailing silence is inaudible and keeps every packet a full frame.
        let frame: Vec<i16> = if chunk.len() == FRAME_SAMPLES {
            chunk.to_vec()
        } else {
            let mut f = chunk.to_vec();
            f.resize(FRAME_SAMPLES, 0);
            f
        };
        let n = enc.encode(&frame, &mut buf).expect("encode");
        out.extend((n as u16).to_le_bytes());
        out.extend(&buf[..n]);
        packets += 1;
    }
    std::fs::write(&outp, &out).expect("write output");
    eprintln!("promptenc: {} packets, {} bytes → {}", packets, out.len(), outp);
}

/// Minimal canonical-WAV reader: expects RIFF/WAVE, pcm_s16le @ 48kHz, 1-2 channels; walks chunks to `fmt ` + `data`. Stereo averages to mono.
fn wav_to_mono_48k(wav: &[u8]) -> Vec<i16> {
    assert!(wav.len() > 44 && &wav[..4] == b"RIFF" && &wav[8..12] == b"WAVE", "not a WAV");
    let (mut channels, mut rate, mut bits) = (0u16, 0u32, 0u16);
    let mut data: Option<&[u8]> = None;
    let mut p = 12usize;
    while p + 8 <= wav.len() {
        let id = &wav[p..p + 4];
        let len = u32::from_le_bytes(wav[p + 4..p + 8].try_into().unwrap()) as usize;
        let body = &wav[p + 8..(p + 8 + len).min(wav.len())];
        match id {
            b"fmt " => {
                channels = u16::from_le_bytes(body[2..4].try_into().unwrap());
                rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                bits = u16::from_le_bytes(body[14..16].try_into().unwrap());
            }
            b"data" => data = Some(body),
            _ => {}
        }
        p += 8 + len + (len & 1); // chunks are word-aligned
    }
    assert_eq!(rate, 48_000, "master must be 48kHz (got {rate})");
    assert_eq!(bits, 16, "master must be s16 (got {bits})");
    assert!((1..=2).contains(&channels), "mono or stereo only");
    let data = data.expect("no data chunk");
    let samples: Vec<i16> = data
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    if channels == 1 {
        samples
    } else {
        samples
            .chunks_exact(2)
            .map(|lr| ((lr[0] as i32 + lr[1] as i32) / 2) as i16)
            .collect()
    }
}
