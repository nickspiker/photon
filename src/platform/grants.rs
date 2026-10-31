//! THE GRANTS GATE (Nick 2026-10-31: "when you initiate a beam or wave it should check all permissions first and ask you to grant before it even starts… not sure why it got built backwards"): placing or answering a wave needs the microphone, a beam the camera too, and nothing is sent until each is granted.
//! A grant not yet asked raises the system prompt and the wave waits on the answer — an edge, never a poll: macOS's completion handler and Android's permission result each bump [`answered`] and wake the app, whose tick re-runs the gate. A grant refused stops the wave there: nothing is sent, the settings pane where the switch lives is opened, and the screen says which grant is off.
//! Linux and Windows have no grants to ask for; every need reads Unknown and passes.

use super::mic_permission::MicAccess;
use std::sync::atomic::{AtomicU64, Ordering};

/// What a wave or beam needs from the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Mic,
    Camera,
}

/// Prompt answers seen this run: the gate holds a wave against the count it saw, and any newer answer re-runs it.
static ANSWERS: AtomicU64 = AtomicU64::new(0);
static WAKE: std::sync::OnceLock<std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>> = std::sync::OnceLock::new();

/// The app's wake handle, so a prompt answered on a system thread runs the gate at once.
pub fn set_wake(proxy: std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>) {
    let _ = WAKE.set(proxy);
}

pub fn answered() -> u64 {
    ANSWERS.load(Ordering::SeqCst)
}

/// A prompt was answered (or the grants were reported afresh): count it and wake the app.
pub fn note_answer() {
    ANSWERS.fetch_add(1, Ordering::SeqCst);
    if let Some(w) = WAKE.get() {
        let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
    }
}

/// The phone's grants as Kotlin last reported them: 0 not reported yet, 1 granted, 2 not granted (a prompt can still ask), 3 refused at a prompt this run (or never shown again).
#[cfg(target_os = "android")]
static ANDROID_MIC: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
#[cfg(target_os = "android")]
static ANDROID_CAMERA: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Kotlin's report (at every resume and after every prompt), then the edge.
#[cfg(target_os = "android")]
pub fn android_report(mic: u8, camera: u8) {
    ANDROID_MIC.store(mic, Ordering::SeqCst);
    ANDROID_CAMERA.store(camera, Ordering::SeqCst);
    note_answer();
}

pub fn status(need: Need) -> MicAccess {
    #[cfg(target_os = "android")]
    {
        let v = match need {
            Need::Mic => ANDROID_MIC.load(Ordering::SeqCst),
            Need::Camera => ANDROID_CAMERA.load(Ordering::SeqCst),
        };
        return match v {
            1 => MicAccess::Granted,
            2 => MicAccess::Undetermined,
            3 => MicAccess::Denied,
            _ => MicAccess::Unknown,
        };
    }
    #[cfg(not(target_os = "android"))]
    match need {
        Need::Mic => super::mic_permission::status(),
        Need::Camera => super::mic_permission::camera_status(),
    }
}

/// Raise the system prompt for one need.
pub fn request(need: Need) {
    crate::logf!("GRANTS: asking for the {}", name(need));
    #[cfg(target_os = "android")]
    {
        let _ = crate::platform::jni_android::wave_service_void(match need {
            Need::Mic => "askMic",
            Need::Camera => "askCameraEarly",
        });
    }
    #[cfg(not(target_os = "android"))]
    match need {
        Need::Mic => super::mic_permission::request(),
        Need::Camera => super::mic_permission::request_camera(),
    }
}

/// Open the settings pane where the refused switch lives (macOS: once per run per need, the existing behaviour; Android: the app's own settings page).
pub fn open_settings(need: Need) {
    #[cfg(target_os = "android")]
    {
        let _ = need;
        let _ = crate::platform::jni_android::wave_service_void("openAppSettings");
    }
    #[cfg(not(target_os = "android"))]
    match need {
        Need::Mic => {
            super::mic_permission::open_settings_once();
        }
        Need::Camera => {
            super::mic_permission::open_camera_settings_once();
        }
    }
}

pub fn name(need: Need) -> &'static str {
    match need {
        Need::Mic => "microphone",
        Need::Camera => "camera",
    }
}
