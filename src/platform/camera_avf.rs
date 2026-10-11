//! The desktop camera on macOS for beams (docs/beams.md stage 5): AVFoundation capture thru objc2 — an AVCaptureSession at 640×480, an AVCaptureVideoDataOutput asking for full-range NV12 ('420f'), and a sample-buffer delegate on its own dispatch queue that converts each frame to planar I420 in integer and hands it to the beam sender as a [`FrameSource`].
//!
//! A Mac's camera pipeline is not ours to address — AVFoundation exposes no white balance, curve or matrix on a built-in or USB camera — so the frames are labelled `creative` / `assumed` / `srgb` and the receiver treats them as such; a chameleon scan of the camera can replace the entry later.
//!
//! The camera grant follows the microphone's pattern (platform/mic_permission.rs): undetermined → the system prompt; denied → System Settings opens on the Camera pane. macos/Info.plist carries NSCameraUsageDescription so the prompt can fire for the bare binary too.

use crate::wave::beam::Gains;
use crate::wave::beam_session::FrameSource;
use crate::wave::h264::{nv12_to_i420, I420};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{class, define_class, msg_send, AllocAnyThread};
use objc2_foundation::NSString;
use std::ffi::c_void;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Mutex;

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}
#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetImageBuffer(sbuf: *mut c_void) -> *mut c_void;
}
#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: *const c_void;
    fn CVPixelBufferLockBaseAddress(pb: *mut c_void, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(pb: *mut c_void, flags: u64) -> i32;
    fn CVPixelBufferGetWidth(pb: *mut c_void) -> usize;
    fn CVPixelBufferGetHeight(pb: *mut c_void) -> usize;
    fn CVPixelBufferGetPixelFormatType(pb: *mut c_void) -> u32;
    fn CVPixelBufferGetBaseAddressOfPlane(pb: *mut c_void, plane: usize) -> *const u8;
    fn CVPixelBufferGetBytesPerRowOfPlane(pb: *mut c_void, plane: usize) -> usize;
}
extern "C" {
    fn dispatch_queue_create(label: *const std::ffi::c_char, attr: *mut c_void) -> *mut c_void;
}

/// '420f' — bi-planar Y + interleaved CbCr, full range.
const NV12_FULL: u32 = u32::from_be_bytes(*b"420f");
/// '420v' — the same, video range (what some cameras insist on).
const NV12_VIDEO: u32 = u32::from_be_bytes(*b"420v");
const READ_ONLY: u64 = 1;

/// Frames from the delegate's queue to the sender thread. Bounded at two: a stalled encoder drops frames at the camera, never queues them.
static FRAME_TX: Mutex<Option<SyncSender<I420>>> = Mutex::new(None);

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PhotonBeamCameraDelegate"]
    struct Delegate;

    impl Delegate {
        /// AVCaptureVideoDataOutputSampleBufferDelegate: one frame. Runs on the capture queue.
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn did_output(&self, _output: *mut AnyObject, sbuf: *mut c_void, _conn: *mut AnyObject) {
            let Some(frame) = (unsafe { nv12_frame(sbuf) }) else { return };
            if let Some(tx) = FRAME_TX.lock().unwrap().as_ref() {
                let _ = tx.try_send(frame);
            }
        }
    }
);

/// Read one CMSampleBuffer's pixel buffer as I420. `None` when it is not the bi-planar 4:2:0 we asked for.
unsafe fn nv12_frame(sbuf: *mut c_void) -> Option<I420> {
    let pb = CMSampleBufferGetImageBuffer(sbuf);
    if pb.is_null() {
        return None;
    }
    let fmt = CVPixelBufferGetPixelFormatType(pb);
    if fmt != NV12_FULL && fmt != NV12_VIDEO {
        return None;
    }
    if CVPixelBufferLockBaseAddress(pb, READ_ONLY) != 0 {
        return None;
    }
    let w = CVPixelBufferGetWidth(pb) & !1;
    let h = CVPixelBufferGetHeight(pb) & !1;
    let y = CVPixelBufferGetBaseAddressOfPlane(pb, 0);
    let ys = CVPixelBufferGetBytesPerRowOfPlane(pb, 0);
    let c = CVPixelBufferGetBaseAddressOfPlane(pb, 1);
    let cs = CVPixelBufferGetBytesPerRowOfPlane(pb, 1);
    let out = if y.is_null() || c.is_null() || w < 16 || h < 16 {
        None
    } else {
        let yplane = std::slice::from_raw_parts(y, ys * h);
        let cplane = std::slice::from_raw_parts(c, cs * (h / 2));
        let mut f = I420::new(w, h);
        nv12_to_i420(yplane, ys, cplane, cs, w, h, fmt == NV12_VIDEO, &mut f);
        Some(f)
    };
    CVPixelBufferUnlockBaseAddress(pb, READ_ONLY);
    out
}

pub struct Camera {
    session: Retained<AnyObject>,
    _delegate: Retained<Delegate>,
    rx: Receiver<I420>,
    w: usize,
    h: usize,
}

// SAFETY: AVCaptureSession's start/stop are documented thread-safe; the delegate is only messaged by AVFoundation on its own queue; the struct is moved to the sender thread once and used there alone.
unsafe impl Send for Camera {}

impl Drop for Camera {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![&*self.session, stopRunning];
        }
        *FRAME_TX.lock().unwrap() = None;
        crate::log("BEAM: Mac camera stopped");
    }
}

/// Open the default camera at 640×480. Blocks briefly for the first frame so the geometry is the camera's real one.
pub fn open() -> Result<Camera, String> {
    match crate::platform::mic_permission::camera_status() {
        crate::platform::mic_permission::MicAccess::Granted => {}
        crate::platform::mic_permission::MicAccess::Undetermined => {
            crate::platform::mic_permission::request_camera();
            return Err(ASKING.into());
        }
        crate::platform::mic_permission::MicAccess::Denied => {
            crate::platform::mic_permission::open_camera_settings_once();
            return Err("BEAM: the camera is off for Photon — System Settings › Privacy & Security › Camera".into());
        }
        crate::platform::mic_permission::MicAccess::Unknown => {}
    }
    unsafe {
        let video = NSString::from_str("vide");
        let device: Option<Retained<AnyObject>> = msg_send![class!(AVCaptureDevice), defaultDeviceWithMediaType: &*video];
        let device = device.ok_or("BEAM: no camera on this Mac")?;
        let null_err: *mut *mut AnyObject = std::ptr::null_mut();
        let input: Option<Retained<AnyObject>> = msg_send![class!(AVCaptureDeviceInput), deviceInputWithDevice: &*device, error: null_err];
        let input = input.ok_or("BEAM: the camera refused to open (another app holding it?)")?;
        let session: Retained<AnyObject> = msg_send![class!(AVCaptureSession), new];
        let preset = NSString::from_str("AVCaptureSessionPreset640x480");
        let ok: bool = msg_send![&*session, canSetSessionPreset: &*preset];
        if ok {
            let _: () = msg_send![&*session, setSessionPreset: &*preset];
        }
        let can_in: bool = msg_send![&*session, canAddInput: &*input];
        if !can_in {
            return Err("BEAM: the capture session would not take the camera".into());
        }
        let _: () = msg_send![&*session, addInput: &*input];
        let output: Retained<AnyObject> = msg_send![class!(AVCaptureVideoDataOutput), new];
        let fmt_num: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithUnsignedInt: NV12_FULL];
        let key = kCVPixelBufferPixelFormatTypeKey as *const AnyObject;
        let settings: Retained<AnyObject> = msg_send![class!(NSDictionary), dictionaryWithObject: &*fmt_num, forKey: key];
        let _: () = msg_send![&*output, setVideoSettings: &*settings];
        let _: () = msg_send![&*output, setAlwaysDiscardsLateVideoFrames: true];
        let delegate: Retained<Delegate> = msg_send![Delegate::alloc(), init];
        let queue = dispatch_queue_create(c"photon.beam.camera".as_ptr(), std::ptr::null_mut());
        let _: () = msg_send![&*output, setSampleBufferDelegate: &*delegate, queue: queue];
        let can_out: bool = msg_send![&*session, canAddOutput: &*output];
        if !can_out {
            return Err("BEAM: the capture session would not take the frame output".into());
        }
        let _: () = msg_send![&*session, addOutput: &*output];
        let (tx, rx) = sync_channel::<I420>(2);
        *FRAME_TX.lock().unwrap() = Some(tx);
        let _: () = msg_send![&*session, startRunning];
        // The first frame names the real geometry (a preset is a request).
        let first = rx.recv_timeout(std::time::Duration::from_secs(5)).map_err(|_| "BEAM: the camera delivered no frame in 5 s".to_string())?;
        let (w, h) = (first.w, first.h);
        crate::logf!("BEAM: Mac camera up — {}×{}, NV12 → I420, labelled creative/assumed/srgb", w, h);
        Ok(Camera { session, _delegate: delegate, rx, w, h })
    }
}

/// The error text `open` returns while the system prompt is up — the Beam path waits for the answer rather than failing.
pub const ASKING: &str = "BEAM: asking for the camera";

impl FrameSource for Camera {
    fn dimensions(&self) -> (usize, usize) {
        (self.w, self.h)
    }
    fn fps(&self) -> u32 {
        30
    }
    fn characterization(&self) -> vsf::spectral_image::ProfileEntry {
        crate::wave::beam_session::assumed_srgb_entry()
    }
    fn next(&mut self) -> Option<(I420, Gains)> {
        loop {
            let f = self.rx.recv_timeout(std::time::Duration::from_secs(5)).ok()?;
            if f.w == self.w && f.h == self.h {
                return Some((f, Gains::NONE));
            }
        }
    }
}
