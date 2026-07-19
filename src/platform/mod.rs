#[cfg(target_os = "android")]
pub mod jni_android;

#[cfg(not(target_os = "android"))]
pub mod autostart;
#[cfg(not(target_os = "android"))]
pub mod control;
#[cfg(not(target_os = "android"))]
pub mod desktop_notify;
