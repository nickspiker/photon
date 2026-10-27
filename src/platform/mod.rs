#[cfg(target_os = "android")]
pub mod jni_android;

#[cfg(not(any(target_os = "android", target_os = "ferros")))]
pub mod autostart;
#[cfg(not(any(target_os = "android", target_os = "ferros")))]
pub mod control;
#[cfg(not(any(target_os = "android", target_os = "ferros")))]
pub mod desktop_notify;
#[cfg(not(any(target_os = "android", target_os = "ferros")))]
pub mod tray;

/// Native-fault catcher (SEH / unix signals) — writes the panic hook's crash sidecar so segfaults ride the next log submission.
pub mod crash_native;

/// OS-locale sniff for the first-launch language seed (docs/languages.md) — the setting is the user's after that.
pub mod locale;

/// Headless-lifeline enrolment (docs/headless-lifeline.md) — the bulletproof-bridge checkbox's platform half: systemd user unit (Linux) / KeepAlive LaunchAgent (macOS).
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod lifeline;

/// Hold off IDLE sleep while a ceremony or transfer is genuinely in flight — scoped to the work by a guard, never to the app (macOS; a no-op elsewhere).
pub mod stay_awake;

/// Wave audio I/O — capture/playback queues for waves (docs/waves.md). Desktop: cpal on a dedicated thread; Android: AAudio streams owned by Rust (audio_aaudio) feeding the same queues with HAL-stamped frames.
pub mod audio;
#[cfg(target_os = "android")]
pub mod audio_aaudio;
/// macOS: Apple's Voice Processing I/O unit in front of the mic (echo cancellation with its gain control off), chosen per route by `audio::voice_dsp_wanted`.
#[cfg(target_os = "macos")]
pub mod audio_vpio;
/// chameleon (VERICHROME's target scanner, closed source) opened at runtime when a shipped build carries it — never linked (docs/beams.md §5d).
pub mod chameleon_plugin;
/// The microphone grant on macOS: ask, say, open Settings (field 2026-10-10, Jon's silent Mac); a stub elsewhere.
pub mod mic_permission;
/// macOS: the desktop camera for beams (AVFoundation, NV12 → I420) — docs/beams.md stage 5.
#[cfg(target_os = "macos")]
pub mod camera_avf;
/// Linux: the desktop camera for beams (V4L2, YUYV → I420) — docs/beams.md stage 5.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub mod camera_v4l2;

/// How long the device has been up, for a log line ("3d 4h", "17m"): CLOCK_BOOTTIME on Linux and Android (counts deep sleep), "unknown" elsewhere.
pub fn uptime_text() -> String {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: a valid timespec for the kernel to fill.
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) } == 0 {
            let s = ts.tv_sec.max(0) as u64;
            return if s >= 86_400 { format!("{}d {}h", s / 86_400, s % 86_400 / 3600) } else if s >= 3600 { format!("{}h {}m", s / 3600, s % 3600 / 60) } else { format!("{}m", s / 60) };
        }
    }
    "unknown".into()
}

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

/// The person's notification preferences (Settings → Notifications and the Wave page), mirrored here as process-wide flags so every notifier honours them from any thread: the new-message chime, the new-message vibration, the incoming-wave vibration. All default ON (the compiled defaults).
pub mod notify_prefs {
    use std::sync::atomic::{AtomicBool, Ordering};
    static CHIME: AtomicBool = AtomicBool::new(true);
    static VIBRATE_MSG: AtomicBool = AtomicBool::new(true);
    static VIBRATE_WAVE: AtomicBool = AtomicBool::new(true);

    pub fn set(chime: bool, vibrate_msg: bool, vibrate_wave: bool) {
        CHIME.store(chime, Ordering::Relaxed);
        VIBRATE_MSG.store(vibrate_msg, Ordering::Relaxed);
        VIBRATE_WAVE.store(vibrate_wave, Ordering::Relaxed);
    }
    pub fn chime() -> bool {
        CHIME.load(Ordering::Relaxed)
    }
    pub fn vibrate_msg() -> bool {
        VIBRATE_MSG.load(Ordering::Relaxed)
    }
    pub fn vibrate_wave() -> bool {
        VIBRATE_WAVE.load(Ordering::Relaxed)
    }
}

/// A thread's standing with the OS scheduler, where the OS has one.
#[derive(Clone, Copy)]
pub enum Priority {
    Min,
    Max,
}

/// `f` on a named thread at `priority` — thread_priority on the desktops and Android; ferros has no thread priorities yet, so there the name alone.
pub fn spawn_at<F: FnOnce() + Send + 'static>(name: &str, priority: Priority, f: F) {
    #[cfg(not(target_os = "ferros"))]
    {
        use thread_priority::{ThreadBuilderExt, ThreadPriority};
        let p = match priority {
            Priority::Min => ThreadPriority::Min,
            Priority::Max => ThreadPriority::Max,
        };
        std::thread::Builder::new().name(name.to_string()).spawn_with_priority(p, move |_| f()).unwrap_or_else(|e| panic!("spawning the {name} thread: {e}"));
    }
    #[cfg(target_os = "ferros")]
    {
        let _ = priority;
        std::thread::Builder::new().name(name.to_string()).spawn(f).unwrap_or_else(|e| panic!("spawning the {name} thread: {e}"));
    }
}

/// Drop the calling thread to the lowest OS priority, where the OS has one.
pub fn lower_this_thread() {
    #[cfg(not(target_os = "ferros"))]
    let _ = thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Min);
}

// The sockets Photon talks through: tokio's where a kernel offers sockets, the types over ferros's ip server on the phone.
pub mod net;
#[cfg(target_os = "ferros")]
pub mod ferros_net;

// ferros: no window system, no tray, no login items, no local control socket — the display server owns the panel and the shell decides what runs at boot. These are the desktop modules' answers on the phone.
#[cfg(target_os = "ferros")]
pub mod desktop_notify {
    pub fn set_window_visible(_visible: bool) {}

    pub fn set_window_focused(_focused: bool) {}

    /// The app in front is the screen: attended.
    pub fn window_attended() -> bool {
        true
    }

    pub fn set_attention_ours(_ours: bool) {}

    pub fn set_wake(_proxy: std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>) {}

    pub fn take_open_conversation() -> Option<[u8; 32]> {
        None
    }

    pub fn notify_new_message(_msg_hp: &[u8; 32], _sender: &str, _text: &str, _conversation: Option<[u8; 32]>) {}
}

#[cfg(target_os = "ferros")]
pub mod autostart {
    pub fn background_desired() -> bool {
        false
    }

    pub fn set_background_desired(_on: bool) {}

    pub fn ensure_enrolled() {}

    pub fn enabled() -> bool {
        false
    }

    pub fn enable() -> Result<(), String> {
        Err("the shell decides what runs at boot on ferros".to_string())
    }

    pub fn disable() -> Result<(), String> {
        Err("the shell decides what runs at boot on ferros".to_string())
    }
}

#[cfg(target_os = "ferros")]
pub mod tray {
    pub fn spawn(_proxy: std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>) {}
}

#[cfg(target_os = "ferros")]
pub mod control {
    /// No second instance can knock on ferros: the shell runs one Photon.
    pub fn spawn_accept_thread(_proxy: std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>) {}
}
