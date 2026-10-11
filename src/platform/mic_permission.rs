//! The microphone grant on macOS (field 2026-10-10, Jon's MacBook: a wave whose capture ran at full rate and delivered zeros for 28 s — macOS hands SILENCE, never an error, when the app has no Microphone grant). Nick: "is there a way to trigger a popup or something so grandma can just use Photon?" — there is, and this asks for it on purpose.
//!
//! Three moves, all small: ASK — when the status is undetermined, request access, which raises the system prompt ("Photon would like to access the microphone", Allow) over the wave screen; SAY — when it is denied, the wave screen says so, because macOS never re-prompts after a denial; OPEN — the one thing she can do is flip Photon's switch in System Settings › Privacy & Security › Microphone, so that pane is opened for her, once per session.
//!
//! The grant is keyed to the binary's code identity and the usage description it carries: build.rs embeds macos/Info.plist (NSMicrophoneUsageDescription) into the bare Mach-O as a __TEXT,__info_plist section, and sign.sh signs Apple targets with the stable self-signed identity, so a grant survives updates. Everywhere but macOS this module is a stub that reports Unknown (Android asks thru its own permission flow).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicAccess {
    Granted,
    Denied,
    Undetermined,
    /// No way to ask on this platform (or the query failed): proceed, the capture path decides.
    Unknown,
}

#[cfg(target_os = "macos")]
mod mac {
    use super::MicAccess;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}

    /// AVMediaTypeAudio's value — the four-char tag, so no framework constant needs importing.
    fn audio_type() -> objc2::rc::Retained<NSString> {
        NSString::from_str("soun")
    }

    pub fn status() -> MicAccess {
        let media = audio_type();
        // AVAuthorizationStatus: 0 notDetermined, 1 restricted, 2 denied, 3 authorized.
        let st: isize = unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: &*media] };
        match st {
            0 => MicAccess::Undetermined,
            1 | 2 => MicAccess::Denied,
            3 => MicAccess::Granted,
            _ => MicAccess::Unknown,
        }
    }

    /// Raise the system prompt (a no-op if the status is already decided). The answer lands on a system queue; it is logged, and the capture path sees the grant on its next open.
    pub fn request() {
        let media = audio_type();
        let block = block2::RcBlock::new(|granted: objc2::runtime::Bool| {
            crate::logf!("AUDIO: microphone access {} by the user", if granted.as_bool() { "GRANTED" } else { "DENIED" });
        });
        unsafe {
            let _: () = msg_send![class!(AVCaptureDevice), requestAccessForMediaType: &*media, completionHandler: &*block];
        }
    }

    /// The CAMERA grant (beams, docs/beams.md stage 5) — the same calls with AVMediaTypeVideo ("vide").
    pub fn camera_status() -> MicAccess {
        let media = NSString::from_str("vide");
        let st: isize = unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: &*media] };
        match st {
            0 => MicAccess::Undetermined,
            1 | 2 => MicAccess::Denied,
            3 => MicAccess::Granted,
            _ => MicAccess::Unknown,
        }
    }

    pub fn request_camera() {
        let media = NSString::from_str("vide");
        let block = block2::RcBlock::new(|granted: objc2::runtime::Bool| {
            crate::logf!("BEAM: camera access {} by the user", if granted.as_bool() { "GRANTED" } else { "DENIED" });
        });
        unsafe {
            let _: () = msg_send![class!(AVCaptureDevice), requestAccessForMediaType: &*media, completionHandler: &*block];
        }
    }

    static CAMERA_SETTINGS_OPENED: AtomicBool = AtomicBool::new(false);

    pub fn open_camera_settings_once() -> bool {
        if CAMERA_SETTINGS_OPENED.swap(true, Ordering::Relaxed) {
            return false;
        }
        std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Camera").spawn().is_ok()
    }

    static SETTINGS_OPENED: AtomicBool = AtomicBool::new(false);

    /// Open System Settings on the Microphone privacy pane, once per session — the pane where Photon's switch is.
    pub fn open_settings_once() -> bool {
        if SETTINGS_OPENED.swap(true, Ordering::Relaxed) {
            return false;
        }
        let ok = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
            .spawn()
            .is_ok();
        crate::logf!("AUDIO: microphone denied — System Settings › Privacy & Security › Microphone {}", if ok { "opened for the user" } else { "could not be opened" });
        ok
    }
}

#[cfg(not(target_os = "macos"))]
mod mac {
    use super::MicAccess;
    pub fn status() -> MicAccess {
        MicAccess::Unknown
    }
    pub fn request() {}
    pub fn open_settings_once() -> bool {
        false
    }
    pub fn camera_status() -> MicAccess {
        MicAccess::Unknown
    }
    pub fn request_camera() {}
    pub fn open_camera_settings_once() -> bool {
        false
    }
}

pub use mac::{camera_status, open_camera_settings_once, open_settings_once, request, request_camera, status};

/// The wave-start edge: ask when undetermined, say and open Settings when denied. Returns the status seen, and sets [`crate::wave::MIC_DENIED`] for the wave screen.
pub fn at_wave_start() -> MicAccess {
    let st = status();
    match st {
        MicAccess::Undetermined => {
            crate::log("AUDIO: microphone access undetermined — asking the system for the grant");
            request();
        }
        MicAccess::Denied => {
            open_settings_once();
        }
        _ => {}
    }
    crate::wave::MIC_DENIED.store(st == MicAccess::Denied, std::sync::atomic::Ordering::Relaxed);
    st
}
