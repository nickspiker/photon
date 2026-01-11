// Global debug flag - can be toggled at runtime with Ctrl+D
use std::sync::atomic::AtomicBool;
pub static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Photon network ports - used for ALL network communication
/// UDP: peer-to-peer status pings, CLUTCH ceremony, chat messages
/// TCP: large payloads (full CLUTCH offers ~548KB, KEM responses ~17KB)
/// FGTW: handle registration and peer discovery announcements
/// Primary: 4383, Fallback: 3546 (both IANA unassigned)
pub const PHOTON_PORT: u16 = 4383;
pub const PHOTON_PORT_FALLBACK: u16 = 3546;

/// Multicast port for LAN peer discovery
/// Separate from main port to avoid SO_REUSEADDR complexity
/// 4384 is IANA unassigned
pub const MULTICAST_PORT: u16 = 4384;

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

// Logging - feature-gated, compiles to nothing without --features logging
// - Android: log::info!
// - Windows: %APPDATA%\photon\photon.log
// - Other: stdout

#[cfg(all(feature = "logging", target_os = "windows"))]
static WINDOWS_LOG_FILE: std::sync::OnceLock<std::sync::Mutex<std::fs::File>> =
    std::sync::OnceLock::new();

/// Initialize logging - must be called early in main() on Windows
#[cfg(all(feature = "logging", target_os = "windows"))]
pub fn init_logging() {
    let _ = WINDOWS_LOG_FILE.get_or_init(|| {
        let config_dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let log_dir = config_dir.join("photon");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("photon.log");
        let file = std::fs::File::create(&log_path).expect("Failed to create log file");
        std::sync::Mutex::new(file)
    });
}

#[cfg(not(all(feature = "logging", target_os = "windows")))]
pub fn init_logging() {}

// Disabled: compiles to nothing
#[cfg(not(feature = "logging"))]
#[inline(always)]
pub fn log(_msg: &str) {}

// Enabled: platform-specific output
#[cfg(feature = "logging")]
pub fn log(msg: &str) {
    #[cfg(target_os = "android")]
    log::info!("{}", msg);

    #[cfg(target_os = "windows")]
    {
        use std::io::Write;
        if let Some(file_mutex) = WINDOWS_LOG_FILE.get() {
            if let Ok(mut file) = file_mutex.lock() {
                let _ = writeln!(file, "{}", msg);
                let _ = file.flush();
            }
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "windows")))]
    println!("{}", msg);
}

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
