// Global debug flag - can be toggled at runtime with Ctrl+D
use std::sync::atomic::AtomicBool;
pub static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

// Debug print macro - only prints if DEBUG_ENABLED is true
#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {
        if $crate::DEBUG_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            println!($($arg)*);
        }
    };
}

pub mod crypto;
pub mod logic;
pub mod network;
pub mod platform;
pub mod storage;
pub mod types;
pub mod ui;

pub use types::*;
