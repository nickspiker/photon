// Global debug flag - can be toggled at runtime with Ctrl+D
use std::sync::atomic::AtomicBool;
pub static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Photon network port - used for ALL network communication
/// UDP: peer-to-peer status pings, CLUTCH ceremony, chat messages
/// TCP: large payloads (full CLUTCH offers ~548KB, KEM responses ~17KB)
/// FGTW: handle registration and peer discovery announcements
/// LAN: broadcast discovery for NAT hairpinning workaround
/// Port 4383 is IANA unassigned (range 4380-4388)
pub const PHOTON_PORT: u16 = 4383;

// Debug print macro - only prints if DEBUG_ENABLED is true
// Compiled out entirely in release builds
#[cfg(debug_assertions)]
#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {
        if $crate::DEBUG_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            println!($($arg)*);
        }
    };
}

#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {};
}

// Unified logging - use log:: on Android, println/eprintln on desktop
// Controlled by "logging" feature flag (default on, disable with --no-default-features)

#[cfg(all(target_os = "android", feature = "logging"))]
pub fn log_info(msg: &str) {
    log::info!("{}", msg);
}

#[cfg(all(target_os = "android", not(feature = "logging")))]
#[inline(always)]
pub fn log_info(_msg: &str) {}

#[cfg(all(not(target_os = "android"), feature = "logging"))]
pub fn log_info(msg: &str) {
    println!("{}", msg);
}

#[cfg(all(not(target_os = "android"), not(feature = "logging")))]
#[inline(always)]
pub fn log_info(_msg: &str) {}

#[cfg(all(target_os = "android", feature = "logging"))]
pub fn log_error(msg: &str) {
    log::error!("{}", msg);
}

#[cfg(all(target_os = "android", not(feature = "logging")))]
#[inline(always)]
pub fn log_error(_msg: &str) {}

#[cfg(all(not(target_os = "android"), feature = "logging"))]
pub fn log_error(msg: &str) {
    eprintln!("{}", msg);
}

#[cfg(all(not(target_os = "android"), not(feature = "logging")))]
#[inline(always)]
pub fn log_error(_msg: &str) {}

pub mod crypto;
pub mod network;
pub mod platform;
pub mod storage;
pub mod types;
pub mod ui;

// Re-export commonly used items from submodules
pub use ui::avatar;
pub use ui::display_profile;

pub use types::*;

// Android JNI initialization
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _: *mut std::os::raw::c_void) -> jni::sys::jint {
    // Initialize Android logger with module filtering
    // Filter out noisy cosmic_text and reqwest debug logs
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("photon")
            .with_max_level(log::LevelFilter::Debug)
            .with_filter(
                android_logger::FilterBuilder::new()
                    .parse("debug,cosmic_text=warn,reqwest=warn")
                    .build(),
            ),
    );

    // Set panic hook for better crash diagnostics
    std::panic::set_hook(Box::new(|panic_info| {
        log::error!("PHOTON PANIC: {}", panic_info);
        if let Some(location) = panic_info.location() {
            log::error!("PANIC location: {}:{}", location.file(), location.line());
        }
    }));

    log::info!("Photon JNI loaded (PID: {})", std::process::id());
    jni::sys::JNI_VERSION_1_6
}
