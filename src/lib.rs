// Global debug flag - set to false to disable all debug logging
pub const DEBUG: bool = true;

// Debug print macro - only prints if DEBUG is true
#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {
        if $crate::DEBUG {
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
