#[cfg(target_os = "android")]
pub mod jni_android;

#[cfg(not(target_os = "android"))]
pub mod autostart;
#[cfg(not(target_os = "android"))]
pub mod control;
#[cfg(not(target_os = "android"))]
pub mod desktop_notify;
#[cfg(not(target_os = "android"))]
pub mod tray;

/// Hold off IDLE sleep while a ceremony or transfer is genuinely in flight — scoped to the work by a guard, never to the app (macOS; a no-op elsewhere).
pub mod stay_awake;
