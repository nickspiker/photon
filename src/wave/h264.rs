//! The desktop H.264 codec for beams (docs/beams.md): Cisco OpenH264, C++ built from source by cc, desktop-only — the phones encode and decode in hardware through MediaCodec and never link this.
//!
//! Where the toolchain can build its C++ today: x86_64 Linux, macOS, and Android (decode; the phone encodes thru MediaCodec). Everywhere else this module is the stub below, [`AVAILABLE`] is false and a beam is not offered — aarch64 Linux joins once its cross sysroot carries libstdc++, Windows once the builder has a C++ cross-compiler and a static C++ runtime link. The gate is the same cfg as the dependency in Cargo.toml; keep the two in step.
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

    /// THE NEVER-PAINTED BLOCKS (field 2026-10-14, Nick: "the frame fill is green for missed frames… literally showing the background when something is missing"): the decoder's picture buffers start at all zeros, and a region it has not received yet — the stripes an intra refresh has not reached, the first moment after a size change — comes out as Y = Cb = Cr = 0, which full-range 601 displays as saturated green. No encoder writes that triple (it is green below black), and concealment copies it along unchanged until the refresh paints real pixels there. One byte per 2×2 block (w/2 × h/2, row-major), non-zero = a hole the wave screen shows its background thru; `None` when the picture is whole (the usual case, no allocation kept).
    pub fn unpainted_mask(&self) -> Option<Vec<u8>> {
        let (cw, ch) = (self.w / 2, self.h / 2);
        let mut mask: Option<Vec<u8>> = None;
        for cy in 0..ch {
            for cx in 0..cw {
                let c = cy * cw + cx;
                if self.u[c] != 0 || self.v[c] != 0 {
                    continue;
                }
                let (y0, y1) = (2 * cy * self.w + 2 * cx, (2 * cy + 1) * self.w + 2 * cx);
                if self.y[y0] != 0 || self.y[y0 + 1] != 0 || self.y[y1] != 0 || self.y[y1 + 1] != 0 {
                    continue;
                }
                mask.get_or_insert_with(|| vec![0u8; cw * ch])[c] = 1;
            }
        }
        mask
    }
}

/// NV12 → I420, integer. Video-range input (16..235 luma, 16..240 chroma) is expanded to full range in Q12, truncating — the receive chain is 601 full range.
pub fn nv12_to_i420(y: &[u8], ys: usize, c: &[u8], cs: usize, w: usize, h: usize, video_range: bool, out: &mut I420) {
    let cw = w / 2;
    for row in 0..h {
        let src = &y[row * ys..row * ys + w];
        let dst = &mut out.y[row * w..row * w + w];
        if video_range {
            // (Y − 16) × 255/219 in Q16, the constant rounded UP so video white (235) truncates to exactly 255 (Q12's 4769 landed it on 254).
            for (d, &s) in dst.iter_mut().zip(src) {
                // WHY/PROOF: the camera's video-range luma still carries codes below 16 and above 235 — the floor and the cap are the clip, since an int→int `as u8` WRAPS.
                *d =(((s as i32 - 16).max(0) * 76310) >> 16).min(255) as u8;
            }
        } else {
            dst.copy_from_slice(src);
        }
    }
    for row in 0..h / 2 {
        let src = &c[row * cs..row * cs + w];
        for x in 0..cw {
            let (cb, cr) = (src[x * 2] as i32, src[x * 2 + 1] as i32);
            let (cb, cr) = if video_range {
                // (C − 128) × 255/224 + 128 in Q16, the constant rounded up as for luma.
                (((cb - 128) * 74606 >> 16) + 128, ((cr - 128) * 74606 >> 16) + 128)
            } else {
                (cb, cr)
            };
            // WHY/PROOF: a video-range camera still sends codes outside 16-240, and the expansion carries those past 0-255 — an int→int `as` WRAPS, so the clamp is the clip; as for luma above.
            out.u[row * cw + x] = cb.clamp(0, 255) as u8;
            out.v[row * cw + x] = cr.clamp(0, 255) as u8;
        }
    }
}

/// Keyframe cadence on the desktop encoder, in frames: the one place a burst is tolerated (docs/beams.md stage 1). At 30 fps this is once every 20 s.
pub const KEYFRAME_EVERY: u32 = 600;

#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), target_os = "macos", target_os = "android"))]
mod real {
    use super::I420;
    use openh264::decoder::{Decoder as Dec, DecoderConfig, Flush};
    use openh264::encoder::{
        BitRate, Encoder as Enc, EncoderConfig, FrameRate, IntraFramePeriod, Profile, RateControlMode, UsageType,
    };
    use openh264::formats::YUVSlices;
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

        /// Change the target bitrate LIVE (the beam ladder's within-band step): no keyframe, no new parameter sets.
        pub fn set_bitrate(&mut self, bps: u32) {
            let mut info = openh264_sys2::SBitrateInfo { iLayer: openh264_sys2::SPATIAL_LAYER_ALL, iBitrate: bps.min(i32::MAX as u32) as i32 };
            unsafe {
                self.enc.raw_api().set_option(openh264_sys2::ENCODER_OPTION_BITRATE, &mut info as *mut _ as *mut std::ffi::c_void);
            }
        }

        pub fn dimensions(&self) -> (usize, usize) {
            (self.w, self.h)
        }
    }

    pub struct Decoder {
        dec: Dec,
        /// Pictures delivered with the decoder's concealment at work (a reference was lost and painted over) — the instrument line.
        pub concealed: u64,
    }

    impl Decoder {
        pub fn new() -> Result<Self, String> {
            let cfg = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
            let mut dec = Dec::with_api_config(OpenH264API::from_source(), cfg)
                .map_err(|e| format!("H264: decoder: {e}"))?;
            // ERROR CONCEALMENT ON (field 2026-10-13, Emma's phone: one lost reference and every later frame failed "ref lost + no param sets", forever). The crate leaves it disabled and has no setter; a beam never keyframes (intra refresh), so the decoder must paint over a loss and keep going until the refresh stripe heals it. Slice + motion-vector copy, across IDRs — OpenH264's strongest mode.
            let mut idc: i32 = openh264_sys2::ERROR_CON_SLICE_MV_COPY_CROSS_IDR as i32;
            unsafe {
                dec.raw_api().set_option(openh264_sys2::DECODER_OPTION_ERROR_CON_IDC, &mut idc as *mut i32 as *mut std::ffi::c_void);
            }
            Ok(Self { dec, concealed: 0 })
        }

        /// Decode one access unit (Annex-B). `None` when the unit produced no picture yet (parameter sets alone, or a frame the decoder is still assembling). A picture the decoder had to conceal is still a picture: the raw entry point is used because the crate's wrapper turns every non-zero decoding state — concealment included — into an error and drops the frame.
        pub fn decode(&mut self, au: &[u8]) -> Result<Option<I420>, String> {
            let mut dst = [std::ptr::null_mut::<u8>(); 3];
            let mut info = openh264_sys2::SBufferInfo::default();
            let state = unsafe { self.dec.raw_api().decode_frame_no_delay(au.as_ptr(), au.len() as std::ffi::c_int, dst.as_mut_ptr(), &mut info) };
            if info.iBufferStatus == 1 && !dst[0].is_null() && !dst[1].is_null() && !dst[2].is_null() {
                let sb = unsafe { info.UsrData.sSystemBuffer };
                let (w, h) = (sb.iWidth.max(0) as usize & !1, sb.iHeight.max(0) as usize & !1);
                let (sy, suv) = (sb.iStride[0].max(0) as usize, sb.iStride[1].max(0) as usize);
                if w < 2 || h < 2 || sy < w || suv < w / 2 {
                    return Err(format!("H264: decode: odd picture geometry {w}×{h} strides {sy}/{suv}"));
                }
                let mut out = I420::new(w, h);
                unsafe {
                    copy_plane(std::slice::from_raw_parts(dst[0], sy * h), sy, w, h, &mut out.y);
                    copy_plane(std::slice::from_raw_parts(dst[1], suv * (h / 2)), suv, w / 2, h / 2, &mut out.u);
                    copy_plane(std::slice::from_raw_parts(dst[2], suv * (h / 2)), suv, w / 2, h / 2, &mut out.v);
                }
                if state != 0 {
                    self.concealed += 1;
                }
                return Ok(Some(out));
            }
            if state != 0 {
                return Err(format!("H264: decode state {state} (no picture)"));
            }
            Ok(None)
        }
    }

    fn copy_plane(src: &[u8], stride: usize, w: usize, h: usize, dst: &mut [u8]) {
        for row in 0..h {
            let s = &src[row * stride..row * stride + w];
            dst[row * w..row * w + w].copy_from_slice(s);
        }
    }
}

#[cfg(not(any(all(target_os = "linux", target_arch = "x86_64"), target_os = "macos", target_os = "android")))]
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
        pub fn set_bitrate(&mut self, _bps: u32) {}
        pub fn dimensions(&self) -> (usize, usize) {
            (0, 0)
        }
    }

    pub struct Decoder {
        pub concealed: u64,
    }
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

#[cfg(test)]
mod conv_tests {
    use super::*;

    #[test]
    fn never_painted_blocks_are_masked_and_decoded_ones_are_not() {
        let mut pic = I420::new(4, 4);
        pic.u.iter_mut().for_each(|v| *v = 0);
        pic.v.iter_mut().for_each(|v| *v = 0);
        // One block was really decoded: black luma, real chroma — not a hole.
        pic.u[3] = 128;
        pic.v[3] = 128;
        assert_eq!(pic.unpainted_mask(), Some(vec![1, 1, 1, 0]));
        let whole = I420::new(4, 4);
        assert_eq!(whole.unpainted_mask(), None, "a whole picture (black included) has no mask");
    }

    #[test]
    fn nv12_deinterleaves_and_expands_video_range() {
        let (w, h) = (4, 2);
        let y = [16u8, 235, 16, 235, 128, 128, 128, 128];
        let c = [128u8, 128, 16, 240];
        let mut f = I420::new(w, h);
        nv12_to_i420(&y, 4, &c, 4, w, h, false, &mut f);
        assert_eq!(f.y, y.to_vec());
        assert_eq!(f.u, vec![128, 16]);
        assert_eq!(f.v, vec![128, 240]);
        nv12_to_i420(&y, 4, &c, 4, w, h, true, &mut f);
        assert_eq!(&f.y[..2], &[0, 255]);
        assert_eq!(f.u[0], 128);
    }
}

#[cfg(all(test, any(all(target_os = "linux", target_arch = "x86_64"), target_os = "macos", target_os = "android")))]
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
