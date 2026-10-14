//! CHAMELEON, OPENED AT RUNTIME (Nick 2026-10-14: "on the Chameleon integration, yes. However you want to wire it in"): VERICHROME's target scanner turns one camera frame of a held-up target into a calibrated (`relative`, `unit`) colour matrix — docs/beams.md §5d.
//!
//! chameleon is closed source; photon is not. A Cargo dependency — even an optional one behind a feature — leaves every photon checkout unbuildable without the private sibling (Cargo resolves optional path dependencies for the lockfile; measured 2026-10-14), so photon never links it. The shipped builds carry chameleon's C library beside photon's own (Android: the APK's native libs, scripts/lib/android.sh) and photon opens it when it is there. No library, no Calibrate button; nothing else changes.
//!
//! The one entry point is `chameleon_scan_linear_rgb` (chameleon/src/ffi.rs). The first scan of a target serial downloads that target's spectral data from VERICHROME; later scans of it are offline. Photon hands over no device identifier (on Android chameleon keys its local cache from a random per-install salt instead).

use std::ffi::{c_char, CStr};
use std::sync::OnceLock;

/// What a scan solved: the XYZ→camera matrix (D50, row-major — lumis's ColorMatrix1) and the target it was solved against.
#[derive(Debug, Clone)]
pub struct Scan {
    pub xyz_to_cam: [f32; 9],
    pub target_type: u32,
    pub serial: u64,
    /// chameleon's linearity estimate of the frame (~1.0 = linear, as it must be).
    pub gamma: f32,
    /// Remaining target life, 0..1.
    pub life: f32,
    pub timestamp: String,
    pub warning: String,
}

/// chameleon's `ChameleonCal`, field for field.
#[repr(C)]
struct ChameleonCal {
    success: bool,
    xyz_to_cam: [f32; 9],
    target_type: u32,
    serial: u64,
    gamma: f32,
    life: f32,
    timestamp: *mut c_char,
    warning: *mut c_char,
    error: *mut c_char,
}

type ScanFn = unsafe extern "C" fn(*const f32, u32, u32, *const c_char, *const c_char, *mut ChameleonCal) -> bool;
type FreeFn = unsafe extern "C" fn(*mut c_char);

struct Plugin {
    scan: ScanFn,
    free: FreeFn,
}

static PLUGIN: OnceLock<Option<Plugin>> = OnceLock::new();

/// Is chameleon's library shipped with this build? Opened once, on first ask.
pub fn available() -> bool {
    plugin().is_some()
}

fn plugin() -> Option<&'static Plugin> {
    PLUGIN.get_or_init(open).as_ref()
}

#[cfg(unix)]
fn open() -> Option<Plugin> {
    for path in candidates() {
        let Ok(c) = std::ffi::CString::new(path.clone()) else { continue };
        // SAFETY: dlopen/dlsym with valid C strings; the handle is never closed, so the function pointers stay valid for the process's life.
        unsafe {
            let h = libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if h.is_null() {
                continue;
            }
            let scan = libc::dlsym(h, c"chameleon_scan_linear_rgb".as_ptr());
            let free = libc::dlsym(h, c"chameleon_free_string".as_ptr());
            if scan.is_null() || free.is_null() {
                crate::logf!("CALIBRATE: {} opened but lacks the photon entry point — an older chameleon", path);
                continue;
            }
            crate::logf!("CALIBRATE: chameleon opened from {}", path);
            return Some(Plugin { scan: std::mem::transmute::<*mut libc::c_void, ScanFn>(scan), free: std::mem::transmute::<*mut libc::c_void, FreeFn>(free) });
        }
    }
    None
}

#[cfg(not(unix))]
fn open() -> Option<Plugin> {
    None
}

/// Where a shipped build puts the library: the APK's native lib dir (the linker finds it by name), beside the binary, or in a macOS bundle's Frameworks.
#[cfg(unix)]
fn candidates() -> Vec<String> {
    #[cfg(target_os = "android")]
    {
        vec!["libchameleon.so".into()]
    }
    #[cfg(not(target_os = "android"))]
    {
        let name = if cfg!(target_os = "macos") { "libchameleon.dylib" } else { "libchameleon.so" };
        let mut v = Vec::new();
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
            v.push(dir.join(name).to_string_lossy().into_owned());
            if cfg!(target_os = "macos") {
                v.push(dir.join("../Frameworks").join(name).to_string_lossy().into_owned());
            }
        }
        v
    }
}

/// Scan one linear RGB frame (interleaved, 0..1) for a target. Blocking — a worker thread's call.
pub fn scan(rgb: &[f32], w: usize, h: usize) -> Result<Scan, String> {
    let p = plugin().ok_or("chameleon is not part of this build")?;
    if w == 0 || h == 0 || rgb.len() < w * h * 3 {
        return Err("no frame to scan".into());
    }
    // Android has no home directory: chameleon's settings and target cache live in photon's own files dir.
    #[cfg(target_os = "android")]
    let dir = crate::ui::avatar::get_android_data_dir().and_then(|d| std::ffi::CString::new(d.to_string_lossy().into_owned()).ok());
    #[cfg(not(target_os = "android"))]
    let dir: Option<std::ffi::CString> = None;
    let mut out = ChameleonCal { success: false, xyz_to_cam: [0.; 9], target_type: 0, serial: 0, gamma: 0., life: 0., timestamp: std::ptr::null_mut(), warning: std::ptr::null_mut(), error: std::ptr::null_mut() };
    // SAFETY: the frame outlives the call; `out` is a valid ChameleonCal; every string chameleon returns is freed exactly once below.
    let ok = unsafe { (p.scan)(rgb.as_ptr(), w as u32, h as u32, dir.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()), std::ptr::null(), &mut out) };
    let take = |s: *mut c_char| -> String {
        if s.is_null() {
            return String::new();
        }
        let t = unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned();
        unsafe { (p.free)(s) };
        t
    };
    let (timestamp, warning, error) = (take(out.timestamp), take(out.warning), take(out.error));
    if !ok || !out.success {
        return Err(if error.is_empty() { "no target found".into() } else { error });
    }
    Ok(Scan { xyz_to_cam: out.xyz_to_cam, target_type: out.target_type, serial: out.serial, gamma: out.gamma, life: out.life, timestamp, warning })
}
