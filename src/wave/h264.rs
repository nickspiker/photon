//! The desktop H.264 codec for beams (docs/beams.md): Cisco OpenH264, C++ built from source by cc, desktop-only — the phones encode and decode in hardware through MediaCodec and never link this.
//!
//! Where the toolchain can build its C++ today: x86_64 Linux and macOS. Everywhere else this module is the stub below, [`AVAILABLE`] is false and a beam is not offered — aarch64 Linux joins once its cross sysroot carries libstdc++, Windows once the builder has a C++ cross-compiler and a static C++ runtime link, Redox never (the build script has no redox arm). The gate is the same cfg as the dependency in Cargo.toml; keep the two in step.
//!
//! What OpenH264 lacks against the design: a rolling intra refresh (it has only whole IDR frames, periodic or on demand) and a per-frame byte cap (only per-NAL slicing). So the DESKTOP sends a keyframe every [`KEYFRAME_EVERY`] frames and on request, which docs/beams.md allows — the desktop is never the constrained end — and the phone side keeps the no-keyframes rule.
//!
//! Frames cross this boundary as planar I420 ([`I420`]): what the camera paths produce and what the receive chain in docs/beams.md §4 consumes. The codec sees bytes; the colour story (γ2, gains, the characterization entry) lives beside the frame, never in it.

/// Planar 8-bit 4:2:0: Y full size, U and V each (w/2)×(h/2), rows packed (stride = width). Width and height are even.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I420 {
    pub w: usize,
    pub h: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

impl I420 {
    pub fn new(w: usize, h: usize) -> Self {
        let (cw, ch) = (w / 2, h / 2);
        Self { w, h, y: vec![0; w * h], u: vec![128; cw * ch], v: vec![128; cw * ch] }
    }
}

/// Keyframe cadence on the desktop encoder, in frames: the one place a burst is tolerated (docs/beams.md stage 1). At 30 fps this is once every 20 s.
pub const KEYFRAME_EVERY: u32 = 600;

#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), target_os = "macos"))]
mod real {
    use super::I420;
    use openh264::decoder::{Decoder as Dec, DecoderConfig, Flush};
    use openh264::encoder::{
        BitRate, Encoder as Enc, EncoderConfig, FrameRate, IntraFramePeriod, Profile, RateControlMode, UsageType,
    };
    use openh264::formats::{YUVSlices, YUVSource};
    use openh264::{OpenH264API, Timestamp};

    pub const AVAILABLE: bool = true;

    pub struct Encoder {
        enc: Enc,
        w: usize,
        h: usize,
        fps: u32,
        frames: u64,
    }

    impl Encoder {
        /// A real-time camera encoder: constant bitrate, no frame skipping (the loss loop owns the rate, not the encoder), baseline profile, keyframes every [`super::KEYFRAME_EVERY`] frames.
        pub fn new(w: usize, h: usize, fps: u32, bitrate_bps: u32) -> Result<Self, String> {
            if w == 0 || h == 0 || w % 2 != 0 || h % 2 != 0 {
                return Err(format!("H264: frame size {}×{} must be even and non-zero", w, h));
            }
            let cfg = EncoderConfig::new()
                .usage_type(UsageType::CameraVideoRealTime)
                .rate_control_mode(RateControlMode::Bitrate)
                .bitrate(BitRate::from_bps(bitrate_bps))
                .max_frame_rate(FrameRate::from_hz(fps as f32))
                .skip_frames(false)
                .profile(Profile::Baseline)
                .intra_frame_period(IntraFramePeriod::from_num_frames(super::KEYFRAME_EVERY));
            let enc = Enc::with_api_config(OpenH264API::from_source(), cfg)
                .map_err(|e| format!("H264: encoder: {e}"))?;
            Ok(Self { enc, w, h, fps, frames: 0 })
        }

        /// Encode one frame; the bytes are this access unit's Annex-B NAL units (SPS/PPS ride the keyframes). Empty when the encoder emitted nothing for the frame.
        pub fn encode(&mut self, frame: &I420) -> Result<Vec<u8>, String> {
            if frame.w != self.w || frame.h != self.h {
                return Err(format!("H264: frame {}×{} on a {}×{} encoder", frame.w, frame.h, self.w, self.h));
            }
            let src = YUVSlices::new((&frame.y, &frame.u, &frame.v), (self.w, self.h), (self.w, self.w / 2, self.w / 2));
            let ts = Timestamp::from_millis(self.frames * 1000 / self.fps.max(1) as u64);
            self.frames += 1;
            let bs = self.enc.encode_at(&src, ts).map_err(|e| format!("H264: encode: {e}"))?;
            Ok(bs.to_vec())
        }

        /// The next frame is a keyframe — the receiver's recovery ask.
        pub fn force_keyframe(&mut self) {
            self.enc.force_intra_frame();
        }

        pub fn dimensions(&self) -> (usize, usize) {
            (self.w, self.h)
        }
    }

    pub struct Decoder {
        dec: Dec,
    }

    impl Decoder {
        pub fn new() -> Result<Self, String> {
            let cfg = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
            let dec = Dec::with_api_config(OpenH264API::from_source(), cfg)
                .map_err(|e| format!("H264: decoder: {e}"))?;
            Ok(Self { dec })
        }

        /// Decode one access unit (Annex-B). `None` when the unit produced no picture yet (parameter sets alone, or a frame the decoder is still assembling).
        pub fn decode(&mut self, au: &[u8]) -> Result<Option<I420>, String> {
            let pic = self.dec.decode(au).map_err(|e| format!("H264: decode: {e}"))?;
            Ok(pic.map(|p| {
                let (w, h) = p.dimensions();
                let (sy, su, sv) = p.strides();
                let mut out = I420::new(w, h);
                copy_plane(p.y(), sy, w, h, &mut out.y);
                copy_plane(p.u(), su, w / 2, h / 2, &mut out.u);
                copy_plane(p.v(), sv, w / 2, h / 2, &mut out.v);
                out
            }))
        }
    }

    fn copy_plane(src: &[u8], stride: usize, w: usize, h: usize, dst: &mut [u8]) {
        for row in 0..h {
            let s = &src[row * stride..row * stride + w];
            dst[row * w..row * w + w].copy_from_slice(s);
        }
    }
}

#[cfg(not(any(all(target_os = "linux", target_arch = "x86_64"), target_os = "macos")))]
mod real {
    use super::I420;

    pub const AVAILABLE: bool = false;

    pub struct Encoder;
    impl Encoder {
        pub fn new(_w: usize, _h: usize, _fps: u32, _bitrate_bps: u32) -> Result<Self, String> {
            Err("H264: no desktop codec on this platform yet (docs/beams.md stage 1)".into())
        }
        pub fn encode(&mut self, _frame: &I420) -> Result<Vec<u8>, String> {
            Err("H264: no desktop codec on this platform".into())
        }
        pub fn force_keyframe(&mut self) {}
        pub fn dimensions(&self) -> (usize, usize) {
            (0, 0)
        }
    }

    pub struct Decoder;
    impl Decoder {
        pub fn new() -> Result<Self, String> {
            Err("H264: no desktop codec on this platform yet (docs/beams.md stage 1)".into())
        }
        pub fn decode(&mut self, _au: &[u8]) -> Result<Option<I420>, String> {
            Err("H264: no desktop codec on this platform".into())
        }
    }
}

pub use real::{Decoder, Encoder, AVAILABLE};

#[cfg(all(test, any(all(target_os = "linux", target_arch = "x86_64"), target_os = "macos")))]
mod tests {
    use super::*;

    /// A moving gradient with a hard-edged square: enough structure that a broken plane copy or a swapped stride shows up as a PSNR collapse.
    fn frame(w: usize, h: usize, t: usize) -> I420 {
        let mut f = I420::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let inside = x > w / 4 + t && x < w / 2 + t && y > h / 4 && y < h / 2;
                f.y[y * w + x] = if inside { 220 } else { ((x + y + t * 3) % 160) as u8 + 16 };
            }
        }
        for y in 0..h / 2 {
            for x in 0..w / 2 {
                f.u[y * (w / 2) + x] = (96 + (x * 64 / (w / 2))) as u8;
                f.v[y * (w / 2) + x] = (160 - (y * 64 / (h / 2))) as u8;
            }
        }
        f
    }

    fn psnr(a: &[u8], b: &[u8]) -> f64 {
        let mse: f64 = a.iter().zip(b).map(|(x, y)| ((*x as f64) - (*y as f64)).powi(2)).sum::<f64>() / a.len() as f64;
        if mse == 0.0 {
            99.0
        } else {
            10.0 * (255.0f64 * 255.0 / mse).log10()
        }
    }

    #[test]
    fn frames_round_trip_through_the_desktop_codec() {
        assert!(AVAILABLE);
        let (w, h) = (320, 240);
        let mut enc = Encoder::new(w, h, 30, 600_000).unwrap();
        let mut dec = Decoder::new().unwrap();
        let mut decoded = 0;
        let mut first_bytes = 0;
        let mut later_bytes = Vec::new();
        for t in 0..12 {
            let src = frame(w, h, t);
            let au = enc.encode(&src).unwrap();
            assert!(!au.is_empty(), "frame {t} produced no bytes");
            if t == 0 {
                first_bytes = au.len();
            } else {
                later_bytes.push(au.len());
            }
            if let Some(pic) = dec.decode(&au).unwrap() {
                assert_eq!((pic.w, pic.h), (w, h));
                let q = psnr(&src.y, &pic.y);
                assert!(q > 30.0, "frame {t}: luma PSNR {q:.1} dB");
                let qu = psnr(&src.u, &pic.u);
                assert!(qu > 30.0, "frame {t}: chroma PSNR {qu:.1} dB");
                decoded += 1;
            }
        }
        assert!(decoded >= 10, "only {decoded} of 12 frames decoded");
        // The keyframe is the burst; the P-frames that follow are a fraction of it (the whole reason the phone side refuses periodic keyframes).
        let avg_later = later_bytes.iter().sum::<usize>() / later_bytes.len();
        assert!(avg_later < first_bytes, "P-frames ({avg_later} B avg) should be smaller than the keyframe ({first_bytes} B)");
    }

    #[test]
    fn a_forced_keyframe_decodes_cold() {
        let (w, h) = (160, 120);
        let mut enc = Encoder::new(w, h, 15, 200_000).unwrap();
        for t in 0..5 {
            let _ = enc.encode(&frame(w, h, t)).unwrap();
        }
        enc.force_keyframe();
        let au = enc.encode(&frame(w, h, 5)).unwrap();
        // A decoder that saw nothing before must decode this unit on its own: it carries SPS/PPS and an IDR.
        let mut cold = Decoder::new().unwrap();
        let pic = cold.decode(&au).unwrap().expect("a forced keyframe decodes cold");
        assert_eq!((pic.w, pic.h), (w, h));
    }
}
