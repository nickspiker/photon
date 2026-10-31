//! The desktop camera on Linux for beams (docs/beams.md stage 5): V4L2 thru the `v4l` crate, the same road opsin's live path takes. YUYV frames off the driver, converted to planar I420 in integer, handed to the beam sender as a [`FrameSource`].
//! A camera that offers only Motion-JPEG (Nick's Sigma fp, 2026-10-31) is read too: each frame decoded to YCbCr — JPEG's own 601 full range, the range the beam pipeline takes — and shrunk to I420 by whole pixel blocks.
//!
//! A UVC camera's ISP is not ours to address: we pin what the knobs allow (auto white balance off where the driver offers it, so the gains at least hold still) and label the frames `creative` / `assumed` / `srgb` — the receiver inverts the sRGB curve and applies the assumed sRGB→VSF-RGB matrix, and a chameleon scan of the camera can replace the entry later.

use crate::wave::beam::Gains;
use crate::wave::beam_session::FrameSource;
use crate::wave::h264::I420;
use v4l::io::traits::CaptureStream;
use v4l::video::Capture;

const WANT_W: u32 = 640;
const WANT_H: u32 = 480;
const BUFFERS: u32 = 4;
/// The widest frame a Motion-JPEG camera hands the sender: a wider one is shrunk by the smallest power of two that fits (a 3840 frame by 2, to 1920).
const MJPEG_MAX_W: usize = 1 << 11;

/// What the driver delivers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wire {
    Yuyv,
    /// Motion-JPEG at `shrink` source pixels per output pixel on each axis.
    Mjpeg { shrink: usize },
}

pub struct Camera {
    dev: v4l::Device,
    stream: Option<v4l::io::mmap::Stream<'static>>,
    w: usize,
    h: usize,
    fps: u32,
    path: String,
    frames: u64,
    wire: Wire,
}

/// Every /dev/video* that offers YUYV or Motion-JPEG capture, as (path, name) — the name is the driver's card string, what the user knows the camera by. Metadata-only nodes (the second node most UVC cameras expose) have no formats and are skipped.
/// REAL CAMERAS FIRST (2026-10-31: the first listed was an OBS virtual camera nothing fed, so every beam from the desktop failed at its first frame while the Sigma fp sat unused): a v4l2loopback device only carries frames while another program writes into it, so it ranks after every camera a driver owns.
pub fn list() -> Vec<(String, String)> {
    let (yuyv, mjpg) = (v4l::FourCC::new(b"YUYV"), v4l::FourCC::new(b"MJPG"));
    let mut real = Vec::new();
    let mut loopback = Vec::new();
    for i in 0..16u32 {
        let p = format!("/dev/video{i}");
        let Ok(dev) = v4l::Device::with_path(&p) else { continue };
        let Ok(formats) = Capture::enum_formats(&dev) else { continue };
        if formats.iter().any(|f| f.fourcc == yuyv || f.fourcc == mjpg) {
            let caps = dev.query_caps().ok();
            let name = caps.as_ref().map(|c| c.card.clone()).unwrap_or_else(|| p.clone());
            if caps.is_some_and(|c| c.driver == "v4l2 loopback" || c.driver == "v4l2loopback") {
                loopback.push((p, name));
            } else {
                real.push((p, name));
            }
        }
    }
    real.extend(loopback);
    real
}

/// Open a camera (`path`, else the first listed) at 640×480 YUYV, or whatever even size the driver settles on; a camera with no YUYV opens in Motion-JPEG at whatever size it settles on, shrunk to at most MJPEG_MAX_W wide.
pub fn open(path: Option<&str>) -> Result<Camera, String> {
    let cams = list();
    let path = match path.and_then(|want| cams.iter().find(|(p, _)| p == want)) {
        Some((p, _)) => p.clone(),
        None => cams.first().map(|(p, _)| p.clone()).ok_or("BEAM: no V4L2 camera with YUYV or Motion-JPEG capture under /dev/video0..15")?,
    };
    let dev = v4l::Device::with_path(&path).map_err(|e| format!("{path}: {e} (another process has the camera? `fuser {path}`)"))?;
    // Close-on-exec: a child we spawn must not keep the camera busy after we are gone (opsin learned this with ffmpeg).
    {
        unsafe extern "C" {
            fn fcntl(fd: i32, cmd: i32, arg: i32) -> i32;
        }
        unsafe {
            fcntl(dev.handle().fd(), 2, 1); // F_SETFD, FD_CLOEXEC
        }
    }
    let (yuyv, mjpg) = (v4l::FourCC::new(b"YUYV"), v4l::FourCC::new(b"MJPG"));
    let offers_yuyv = Capture::enum_formats(&dev).map_err(|e| e.to_string())?.iter().any(|f| f.fourcc == yuyv);
    let want = if offers_yuyv { yuyv } else { mjpg };
    let mut fmt = Capture::format(&dev).map_err(|e| e.to_string())?;
    fmt.width = WANT_W;
    fmt.height = WANT_H;
    fmt.fourcc = want;
    let fmt = Capture::set_format(&dev, &fmt).map_err(|e| e.to_string())?;
    if fmt.fourcc != want {
        return Err(format!("{path}: wanted {}, got {}", String::from_utf8_lossy(&want.repr), String::from_utf8_lossy(&fmt.fourcc.repr)));
    }
    let (sw, sh) = (fmt.width as usize, fmt.height as usize);
    let wire = if offers_yuyv {
        Wire::Yuyv
    } else {
        let mut shrink = 1;
        while sw / shrink > MJPEG_MAX_W {
            shrink <<= 1;
        }
        Wire::Mjpeg { shrink }
    };
    let s = match wire {
        Wire::Yuyv => 1,
        Wire::Mjpeg { shrink } => shrink,
    };
    let (w, h) = ((sw / s) & !1, (sh / s) & !1);
    if w < 16 || h < 16 {
        return Err(format!("{path}: unusable size {w}×{h}"));
    }
    // Pin the knobs a UVC camera offers so the frames at least hold still: auto white balance off, auto exposure as the driver has it (level is not colour).
    pin_controls(&dev);
    let fps = Capture::params(&dev).ok().and_then(|p| {
        let i = p.interval;
        // WHY/PROOF: the frame interval is the driver's report — a bogus one names 1000 fps or 0, and the beam's frame pacing and the ladder's bitrate are sized for a camera between 5 and 60.
        if i.numerator > 0 { Some((i.denominator / i.numerator).clamp(5, 60)) } else { None }
    }).unwrap_or(30);
    match wire {
        Wire::Yuyv => crate::logf!("BEAM: camera {} — {}×{} YUYV @ {} fps", path, w, h, fps),
        Wire::Mjpeg { shrink } => crate::logf!("BEAM: camera {} — {}×{} Motion-JPEG @ {} fps, shrunk {}× to {}×{}", path, sw, sh, fps, shrink, w, h),
    }
    let mut cam = Camera { dev, stream: None, w, h, fps, path, frames: 0, wire };
    cam.start()?;
    Ok(cam)
}

fn pin_controls(dev: &v4l::Device) {
    // V4L2_CID_AUTO_WHITE_BALANCE = 0x0098090c; a camera without it just errors, which is fine.
    let _ = dev.set_control(v4l::Control { id: 0x0098_090c, value: v4l::control::Value::Boolean(false) });
}

impl Camera {
    fn start(&mut self) -> Result<(), String> {
        // SAFETY of the 'static: the stream borrows the device; both live in this struct and the stream is dropped first (field order below), never outliving `dev`.
        let dev: &'static v4l::Device = unsafe { &*(&self.dev as *const v4l::Device) };
        let stream = v4l::io::mmap::Stream::with_buffers(dev, v4l::buffer::Type::VideoCapture, BUFFERS).map_err(|e| format!("{}: stream: {e}", self.path))?;
        self.stream = Some(stream);
        Ok(())
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        // The stream first, then the device (declared order would drop `dev` first).
        self.stream = None;
    }
}

/// YUYV (Y0 U Y1 V per two pixels) → I420, integer: luma straight thru, each chroma sample the mean of the two rows it covers.
pub fn yuyv_to_i420(src: &[u8], stride: usize, w: usize, h: usize, out: &mut I420) {
    let cw = w / 2;
    for y in 0..h {
        let row = &src[y * stride..y * stride + w * 2];
        let yrow = &mut out.y[y * w..y * w + w];
        for x in 0..w {
            yrow[x] = row[x * 2];
        }
        // Chroma: even rows seed, odd rows average in.
        let crow = (y / 2) * cw;
        if y % 2 == 0 {
            for x in 0..cw {
                out.u[crow + x] = row[x * 4 + 1];
                out.v[crow + x] = row[x * 4 + 3];
            }
        } else {
            for x in 0..cw {
                out.u[crow + x] = ((out.u[crow + x] as u16 + row[x * 4 + 1] as u16) >> 1) as u8;
                out.v[crow + x] = ((out.v[crow + x] as u16 + row[x * 4 + 3] as u16) >> 1) as u8;
            }
        }
    }
}

impl FrameSource for Camera {
    fn dimensions(&self) -> (usize, usize) {
        (self.w, self.h)
    }
    fn fps(&self) -> u32 {
        self.fps
    }
    fn characterization(&self) -> vsf::spectral_image::ProfileEntry {
        crate::wave::beam_session::assumed_srgb_entry()
    }
    fn next(&mut self) -> Option<(I420, Gains)> {
        // A frame that cannot fill the picture (torn, or another size) is skipped and the next one read: the loop ends at a good frame or a camera that has gone.
        loop {
            let stream = self.stream.as_mut()?;
            let (buf, meta) = match stream.next() {
                Ok(x) => x,
                Err(e) => {
                    crate::logf!("BEAM: camera {}: {}", self.path, e);
                    return None;
                }
            };
            let mut out = I420::new(self.w, self.h);
            match self.wire {
                Wire::Yuyv => {
                    let stride = self.w * 2;
                    if buf.len() < stride * self.h {
                        crate::logf!("BEAM: camera {}: short frame {} < {}", self.path, buf.len(), stride * self.h);
                        return None;
                    }
                    yuyv_to_i420(buf, stride, self.w, self.h, &mut out);
                }
                Wire::Mjpeg { shrink } => {
                    // The JPEG is the buffer's first `bytesused` bytes; the rest of the mapped buffer is stale.
                    // WHY/PROOF: bytesused is the driver's word — a driver that reports 0 or more than the buffer would leave the slice empty or past its end; the whole buffer then goes to the decoder, which stops at the JPEG's end marker.
                    let n = meta.bytesused as usize;
                    let jpeg = if n > 0 && n <= buf.len() { &buf[..n] } else { buf };
                    let options = zune_core::options::DecoderOptions::default().jpeg_set_out_colorspace(zune_core::colorspace::ColorSpace::YCbCr);
                    let mut dec = zune_jpeg::JpegDecoder::new_with_options(std::io::Cursor::new(jpeg), options);
                    let ycc = match dec.decode() {
                        Ok(px) => px,
                        Err(e) => {
                            crate::logf!("BEAM: camera {}: a frame did not decode ({}) — skipped", self.path, e);
                            continue;
                        }
                    };
                    let Some((jw, jh)) = dec.dimensions() else {
                        continue;
                    };
                    // WHY/PROOF: the frame is the camera's, external input — one of another size than the stream promised (a driver changing modes under us), or a decode shorter than its header says, would index past the decoded pixels in the shrink.
                    if jw / shrink < self.w || jh / shrink < self.h || ycc.len() < jw * jh * 3 {
                        crate::logf!("BEAM: camera {}: a {}×{} frame where {}×{} was set — skipped", self.path, jw, jh, self.w * shrink, self.h * shrink);
                        continue;
                    }
                    ycc_to_i420_shrunk(&ycc, jw, shrink, self.w, self.h, &mut out);
                }
            }
            self.frames += 1;
            return Some((out, Gains::NONE));
        }
    }
}

/// Interleaved 4:4:4 YCbCr (a decoded JPEG, `src_w` pixels a row) → I420 of `w × h`, each output pixel the mean of the `shrink × shrink` source block it covers and each chroma sample the mean of its 2·shrink square — whole blocks, integer, so the picture is the camera's light averaged, not resampled.
/// One pass, a chroma row (two luma rows) at a time: every source pixel is read once and feeds its luma sum and its chroma sums together.
/// The caller proves `w·shrink ≤ src_w` and `h·shrink` rows exist; the row slices below are cut to exactly the covered pixels.
pub fn ycc_to_i420_shrunk(src: &[u8], src_w: usize, shrink: usize, w: usize, h: usize, out: &mut I420) {
    let cw = w / 2;
    let (area, c_area) = ((shrink * shrink) as u32, (4 * shrink * shrink) as u32);
    let mut acc_y = vec![0u32; w];
    let mut acc_cb = vec![0u32; cw];
    let mut acc_cr = vec![0u32; cw];
    for cy in 0..h / 2 {
        acc_cb.fill(0);
        acc_cr.fill(0);
        for y in [cy * 2, cy * 2 + 1] {
            acc_y.fill(0);
            for dy in 0..shrink {
                let start = (y * shrink + dy) * src_w * 3;
                let row = &src[start..start + w * shrink * 3];
                for (x, block) in row.chunks_exact(3 * shrink).enumerate() {
                    let (mut sy, mut sb, mut sr) = (0u32, 0u32, 0u32);
                    for p in block.chunks_exact(3) {
                        sy += p[0] as u32;
                        sb += p[1] as u32;
                        sr += p[2] as u32;
                    }
                    acc_y[x] += sy;
                    acc_cb[x >> 1] += sb;
                    acc_cr[x >> 1] += sr;
                }
            }
            for (o, s) in out.y[y * w..y * w + w].iter_mut().zip(&acc_y) {
                *o = (s / area) as u8;
            }
        }
        for x in 0..cw {
            out.u[cy * cw + x] = (acc_cb[x] / c_area) as u8;
            out.v[cy * cw + x] = (acc_cr[x] / c_area) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuyv_converts_with_averaged_chroma() {
        let (w, h) = (4, 2);
        // Row 0: pixels Y=10,20,30,40 with U=100 V=200 (pair 0), U=110 V=210 (pair 1); row 1: Y=50..80, U=120 V=220, U=130 V=230.
        let src = vec![
            10, 100, 20, 200, 30, 110, 40, 210, //
            50, 120, 60, 220, 70, 130, 80, 230,
        ];
        let mut out = I420::new(w, h);
        yuyv_to_i420(&src, w * 2, w, h, &mut out);
        assert_eq!(out.y, vec![10, 20, 30, 40, 50, 60, 70, 80]);
        assert_eq!(out.u, vec![110, 120]);
        assert_eq!(out.v, vec![210, 220]);
    }

    /// A Motion-JPEG frame shrunk by two: each luma the mean of its 2×2 block, each chroma the mean of its 4×4.
    #[test]
    fn ycc_shrinks_by_whole_blocks() {
        let (sw, sh) = (8usize, 4usize);
        let src: Vec<u8> = (0..sw * sh).flat_map(|i| [(i * 4) as u8, 100 + (i % sw) as u8, 200]).collect();
        let mut out = I420::new(4, 2);
        ycc_to_i420_shrunk(&src, sw, 2, 4, 2, &mut out);
        // Block (0,0) covers source pixels 0, 1, 8, 9 → luma (0 + 4 + 32 + 36) / 4 = 18.
        assert_eq!(out.y[0], 18);
        // Chroma (0,0) covers columns 0..4 of rows 0..4 → Cb mean 100 + 1.5, truncated to 101.
        assert_eq!(out.u, vec![101, 105]);
        assert_eq!(out.v, vec![200, 200]);
    }
}
