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

/// "Is a human plausibly looking at this app RIGHT NOW" — the platform-appropriate attended check, one name for both worlds (desktop: window visible+focused; Android: Activity foregrounded).
pub fn attended_here() -> bool {
    #[cfg(target_os = "android")]
    {
        jni_android::app_in_foreground()
    }
    #[cfg(not(target_os = "android"))]
    {
        desktop_notify::window_attended()
    }
}
