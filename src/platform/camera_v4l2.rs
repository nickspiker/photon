//! The desktop camera on Linux for beams (docs/beams.md stage 5): V4L2 thru the `v4l` crate, the same road opsin's live path takes. YUYV frames off the driver, converted to planar I420 in integer, handed to the beam sender as a [`FrameSource`].
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

pub struct Camera {
    dev: v4l::Device,
    stream: Option<v4l::io::mmap::Stream<'static>>,
    w: usize,
    h: usize,
    fps: u32,
    path: String,
    frames: u64,
}

/// The first /dev/video* that offers YUYV capture. Metadata-only nodes (the second node most UVC cameras expose) have no formats and are skipped.
fn find_device() -> Result<String, String> {
    let yuyv = v4l::FourCC::new(b"YUYV");
    for i in 0..16u32 {
        let p = format!("/dev/video{i}");
        let Ok(dev) = v4l::Device::with_path(&p) else { continue };
        let Ok(formats) = Capture::enum_formats(&dev) else { continue };
        if formats.iter().any(|f| f.fourcc == yuyv) {
            return Ok(p);
        }
    }
    Err("BEAM: no V4L2 camera with YUYV capture under /dev/video0..15".into())
}

/// Open the camera at 640×480 YUYV, or whatever even size the driver settles on.
pub fn open() -> Result<Camera, String> {
    let path = find_device()?;
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
    let mut fmt = Capture::format(&dev).map_err(|e| e.to_string())?;
    fmt.width = WANT_W;
    fmt.height = WANT_H;
    fmt.fourcc = v4l::FourCC::new(b"YUYV");
    let fmt = Capture::set_format(&dev, &fmt).map_err(|e| e.to_string())?;
    if fmt.fourcc != v4l::FourCC::new(b"YUYV") {
        return Err(format!("{path}: wanted YUYV, got {}", String::from_utf8_lossy(&fmt.fourcc.repr)));
    }
    let (w, h) = (fmt.width as usize & !1, fmt.height as usize & !1);
    if w < 16 || h < 16 {
        return Err(format!("{path}: unusable size {w}×{h}"));
    }
    // Pin the knobs a UVC camera offers so the frames at least hold still: auto white balance off, auto exposure as the driver has it (level is not colour).
    pin_controls(&dev);
    let fps = Capture::params(&dev).ok().and_then(|p| {
        let i = p.interval;
        if i.numerator > 0 { Some((i.denominator / i.numerator).clamp(5, 60)) } else { None }
    }).unwrap_or(30);
    crate::logf!("BEAM: camera {} — {}×{} YUYV @ {} fps", path, w, h, fps);
    let mut cam = Camera { dev, stream: None, w, h, fps, path, frames: 0 };
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
        let stream = self.stream.as_mut()?;
        let (buf, _meta) = match stream.next() {
            Ok(x) => x,
            Err(e) => {
                crate::logf!("BEAM: camera {}: {}", self.path, e);
                return None;
            }
        };
        let stride = self.w * 2;
        if buf.len() < stride * self.h {
            crate::logf!("BEAM: camera {}: short frame {} < {}", self.path, buf.len(), stride * self.h);
            return None;
        }
        let mut out = I420::new(self.w, self.h);
        yuyv_to_i420(buf, stride, self.w, self.h, &mut out);
        self.frames += 1;
        Some((out, Gains::NONE))
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
}
