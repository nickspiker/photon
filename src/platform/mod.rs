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

/// Native-fault catcher (SEH / unix signals) — writes the panic hook's crash sidecar so segfaults ride the next log submission.
pub mod crash_native;

/// OS-locale sniff for the first-launch language seed (docs/languages.md) — the setting is the user's after that.
pub mod locale;

/// Hold off IDLE sleep while a ceremony or transfer is genuinely in flight — scoped to the work by a guard, never to the app (macOS; a no-op elsewhere).
pub mod stay_awake;

/// Call audio I/O — capture/playback queues for voice calls (docs/calls.md). Desktop: cpal on a dedicated thread; Android: Kotlin AudioRecord/AudioTrack across JNI into the same queues.
pub mod audio;

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

/// Is the app the thing on screen right now? Android mirrors the Activity foreground state; desktop counts as always-watching (battery pressure is an order of magnitude lower and window focus is a weaker signal than "screen off"). The demand-driven presence gates (protocol.rs) read this.
pub fn app_watching() -> bool {
    #[cfg(target_os = "android")]
    {
        jni_android::app_in_foreground()
    }
    #[cfg(not(target_os = "android"))]
    {
        true
    }
}
