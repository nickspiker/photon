//! CALIBRATING A BEAM'S CAMERA (Nick 2026-10-14: "on the Chameleon integration, yes. However you want to wire it in") — docs/beams.md §5d.
//!
//! The Calibrate button (beside a phone's running beam) asks for ONE frame: the next full-size camera frame is copied out on its way to the encoder ([`offer_planes`]), turned back into linear camera RGB, and scanned by chameleon on a worker thread (platform::chameleon_plugin). A found target yields the scan's XYZ→camera matrix, which becomes the beam's characterization exactly as lumis makes it: camera → VSF RGB = (XYZ → VSF RGB) × (XYZ → camera)⁻¹, `chameleon` / `relative` / `unit`, illuminant D50 (23) — and the beam re-describes itself at once, the self-view with it.
//!
//! Only on the STRAIGHT path (AWB off, unit gains, identity matrix, our γ2 curve): there the frames are the sensor's own colour and a matrix solved from one of them holds for every later one. With the camera's own white balance running, the next scene would move the colour out from under the matrix, so the button does not show.
//!
//! The calibration lasts for this run of the app (as in lumis): a relative IDT is the camera under the light it was scanned in, and a new room wants a new scan. Each LENS keeps its own (a tap on the self-view cycles cameras mid-beam, and every sensor has its own colour): switching back to a calibrated lens brings its calibration back; a scan that finishes after the camera moved on is dropped, never pasted onto the wrong lens.

use super::beam_scale::Plane;
use super::h264::I420;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// What the wave screen says about calibration.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Idle,
    Scanning,
    Done { serial: u64, warning: String },
    Failed(String),
}

/// The status and the moment it last finished (a finished status shows until the next poke after it).
static STATUS: Mutex<(Status, Option<std::time::Instant>)> = Mutex::new((Status::Idle, None));
/// The next camera frame goes to the scanner.
static WANT: AtomicBool = AtomicBool::new(false);
/// The phone's camera is on the straight path, so a calibration means something.
static STRAIGHT: AtomicBool = AtomicBool::new(false);
type Calibration = (vsf::spectral_image::ProfileEntry, vsf::spectral_image::CalProvenance);
/// This run's calibrations, by lens key ("1", "0/3" — beam_session::android_started's `camera`).
static CALIBRATIONS: Mutex<Vec<(String, Calibration)>> = Mutex::new(Vec::new());
/// The lens the beam is on now.
static CAMERA: Mutex<String> = Mutex::new(String::new());

/// Can the Calibrate button work right now: chameleon shipped, the camera straight?
pub fn available() -> bool {
    STRAIGHT.load(Ordering::Relaxed) && crate::platform::chameleon_plugin::available()
}

/// The phone pipeline came up on `camera` (beam_session::android_started), straight or not.
pub fn set_camera(camera: &str, straight: bool) {
    *CAMERA.lock().unwrap() = camera.to_string();
    STRAIGHT.store(straight, Ordering::Relaxed);
}

/// The phone pipeline's colour path went away (beam_session::android_stop).
pub fn set_straight(on: bool) {
    STRAIGHT.store(on, Ordering::Relaxed);
}

/// The button: scan the next frame. False when it cannot (no chameleon, not straight, a scan already running).
pub fn request() -> bool {
    if !available() {
        return false;
    }
    let mut s = STATUS.lock().unwrap();
    if s.0 == Status::Scanning {
        return false;
    }
    *s = (Status::Scanning, None);
    WANT.store(true, Ordering::Relaxed);
    crate::log("CALIBRATE: scanning the next camera frame for a target");
    true
}

pub fn status() -> (Status, Option<std::time::Instant>) {
    STATUS.lock().unwrap().clone()
}

/// This run's calibration of the lens the beam is on, if one has been made.
pub fn calibration() -> Option<Calibration> {
    let cam = CAMERA.lock().unwrap().clone();
    CALIBRATIONS.lock().unwrap().iter().find(|(k, _)| *k == cam).map(|(_, c)| c.clone())
}

/// Per camera frame on the phone (jni nativeBeamFill), before it is resampled for the encoder: when a scan is wanted, this full-size frame is copied out and handed to the worker. Costs one atomic otherwise.
pub fn offer_planes(src: [Plane; 3]) {
    if !WANT.swap(false, Ordering::Relaxed) {
        return;
    }
    let (w, h) = (src[0].w & !1, src[0].h & !1);
    let camera = CAMERA.lock().unwrap().clone();
    let mut f = I420::new(w, h);
    src[0].copy_to(&mut f.y, w, h);
    src[1].copy_to(&mut f.u, w / 2, h / 2);
    src[2].copy_to(&mut f.v, w / 2, h / 2);
    if let Err(e) = std::thread::Builder::new().name("beam-calibrate".into()).spawn(move || scan_frame(f, camera)) {
        finish(Status::Failed(format!("no scan thread: {e}")));
    }
}

fn finish(s: Status) {
    *STATUS.lock().unwrap() = (s, Some(std::time::Instant::now()));
}

fn scan_frame(f: I420, camera: String) {
    let rgb = linear_rgb(&f);
    match crate::platform::chameleon_plugin::scan(&rgb, f.w, f.h) {
        Ok(scan) => {
            let Some(entry) = entry_from_scan(&scan.xyz_to_cam) else {
                crate::log("CALIBRATE: the scan's matrix is singular — not used");
                finish(Status::Failed("the solved matrix is singular".into()));
                return;
            };
            crate::logf!(
                "CALIBRATE: camera {} — target {} #{} (life {}%, linearity {}) solved {}×{} — relative/unit{}",
                camera,
                scan.target_type,
                scan.serial,
                (scan.life * 100.0).round() as i32,
                (scan.gamma * 100.0).round() / 100.0,
                f.w,
                f.h,
                if scan.warning.is_empty() { String::new() } else { format!(" ⚠ {}", scan.warning.replace('\n', " ")) }
            );
            let cal = vsf::spectral_image::CalProvenance { target_type: scan.target_type, target_serial: scan.serial, timestamp: scan.timestamp.clone() };
            {
                let mut all = CALIBRATIONS.lock().unwrap();
                all.retain(|(k, _)| *k != camera);
                all.push((camera.clone(), (entry.clone(), cal.clone())));
            }
            // Applied live only if the beam is still on the lens the frame came from.
            if *CAMERA.lock().unwrap() == camera {
                super::beam_session::android_apply_calibration(&entry, cal);
            } else {
                crate::logf!("CALIBRATE: kept for camera {} — the beam has moved to another lens since", camera);
            }
            finish(Status::Done { serial: scan.serial, warning: scan.warning });
        }
        Err(e) => {
            crate::logf!("CALIBRATE: no calibration — {}", e);
            finish(Status::Failed(e));
        }
    }
}

/// The frame back to linear camera RGB, interleaved f32 0..1: 601 full-range YCbCr → R'G'B' (the receive chain's own decode), then the straight path's γ2 undone by squaring.
pub fn linear_rgb(f: &I420) -> Vec<f32> {
    let mut out = Vec::with_capacity(f.w * f.h * 3);
    let cw = f.w / 2;
    for y in 0..f.h {
        for x in 0..f.w {
            let l = f.y[y * f.w + x] as f32;
            let c = (y / 2) * cw + x / 2;
            let (cb, cr) = (f.u[c] as f32 - 128.0, f.v[c] as f32 - 128.0);
            for v in [l + 1.402 * cr, l - 0.344_136 * cb - 0.714_136 * cr, l + 1.772 * cb] {
                let e = (v / 255.0).clamp(0.0, 1.0);
                out.push(e * e);
            }
        }
    }
    out
}

/// The beam's entry from a scan's XYZ→camera (lumis's derivation, image/vsf_save.rs `entry`): camera → VSF RGB = (XYZ → VSF RGB) × (XYZ → camera)⁻¹.
pub fn entry_from_scan(xyz_to_cam: &[f32; 9]) -> Option<vsf::spectral_image::ProfileEntry> {
    let cam_to_xyz = super::beam_session::invert3(xyz_to_cam)?;
    Some(vsf::spectral_image::ProfileEntry {
        matrix: super::beam_session::mat_mul3(&super::beam_colour::vsf_rows(vsf::colour::XYZ2VSF_RGB), &cam_to_xyz),
        source: "chameleon".into(),
        class: vsf::spectral_image::IdtClass::Relative,
        tier: vsf::spectral_image::ProfileTier::Unit,
        illuminant: 23, // D50: chameleon writes its ColorMatrix1 there
        transfer: vsf::spectral_image::Transfer::Gamma2,
        scale: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grey_frame_is_grey_and_linear() {
        let mut f = I420::new(4, 2);
        f.y.iter_mut().for_each(|v| *v = 128);
        let rgb = linear_rgb(&f);
        let want = (128.0f32 / 255.0).powi(2);
        assert!(rgb.iter().all(|&v| (v - want).abs() < 1e-6), "γ2 undone: 128 → {want}");
    }

    #[test]
    fn the_entry_is_lumis_relative_unit_and_maps_its_white() {
        // A scan whose XYZ→camera is the identity: camera white = XYZ E white = VSF white.
        let e = entry_from_scan(&[1., 0., 0., 0., 1., 0., 0., 0., 1.]).unwrap();
        assert_eq!((e.class, e.tier, e.source.as_str(), e.illuminant), (vsf::spectral_image::IdtClass::Relative, vsf::spectral_image::ProfileTier::Unit, "chameleon", 23));
        for r in 0..3 {
            let s: f32 = e.matrix[r * 3..r * 3 + 3].iter().sum();
            assert!((s - 1.0).abs() < 2e-3, "row {r} maps white to {s}");
        }
        assert!(entry_from_scan(&[0.; 9]).is_none());
    }

    #[test]
    fn nothing_is_asked_without_chameleon_and_a_straight_camera() {
        set_straight(false);
        assert!(!request());
        assert_eq!(status().0, Status::Idle);
    }
}
