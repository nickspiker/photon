mod app;
mod colour;
mod compositing;
mod keyboard;
mod mouse;
mod text_editing;
mod text_rasterizing;

#[cfg(target_os = "windows")]
mod renderer_windows;

#[cfg(target_os = "linux")]
mod renderer_linux;

#[cfg(target_os = "windows")]
use renderer_windows as renderer;

#[cfg(target_os = "linux")]
use renderer_linux as renderer;

pub mod theme;

pub use app::{HandleStatus, PhotonApp};
