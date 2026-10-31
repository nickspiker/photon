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
        let asked = std::time::Instant::now();
        let block = block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
            // An answer inside half a second was not a person: macOS refused without showing the prompt (the stale-bundle case — see repair_bundle_plist).
            let ms = asked.elapsed().as_millis();
            if !granted.as_bool() && ms < 500 {
                crate::logf!("AUDIO: microphone access refused by macOS WITHOUT a prompt ({} ms) — the app's description was missing or not trusted; repaired at startup, its permission entry reset, so quit and reopen Photon", ms);
                reset_tcc("Microphone");
                crate::wave::MIC_RELAUNCH.store(true, std::sync::atomic::Ordering::Relaxed);
            } else {
                crate::logf!("AUDIO: microphone access {} by the user", if granted.as_bool() { "GRANTED" } else { "DENIED" });
            }
            super::super::grants::note_answer();
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
        let asked = std::time::Instant::now();
        let block = block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
            let ms = asked.elapsed().as_millis();
            if !granted.as_bool() && ms < 500 {
                crate::logf!("BEAM: camera access refused by macOS WITHOUT a prompt ({} ms) — repaired at startup, its permission entry reset; quit and reopen Photon", ms);
                reset_tcc("Camera");
            } else {
                crate::logf!("BEAM: camera access {} by the user", if granted.as_bool() { "GRANTED" } else { "DENIED" });
            }
            super::super::grants::note_answer();
        });
        unsafe {
            let _: () = msg_send![class!(AVCaptureDevice), requestAccessForMediaType: &*media, completionHandler: &*block];
        }
    }

    /// Clear Photon's own entry for one privacy service (the user's database, no admin rights): after a refusal macOS made without asking, the next request asks a person instead of repeating the refusal. Never used on a real "Don't Allow" — that is the person's answer.
    fn reset_tcc(service: &str) {
        let ok = std::process::Command::new("/usr/bin/tccutil").args(["reset", service, "org.fgtw.photon-messenger"]).status().map(|s| s.success()).unwrap_or(false);
        crate::logf!("macOS: permission entry for {} {}", service, if ok { "reset — the next request asks" } else { "could not be reset" });
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

/// THE BUNDLE'S OWN DESCRIPTION (field 2026-10-13, Nick's MacBook on 0.117.0: "microphone access undetermined — asking" then "DENIED by the user" ten milliseconds later, no prompt ever shown). A process launched from `Photon Messenger.app` is judged by the BUNDLE's Info.plist, not the one embedded in the binary — and updates replace only the binary, so a bundle installed before the plist carried a microphone (or camera) description keeps lacking it forever, and macOS refuses privacy access without asking. The bundle's plist also differed from the embedded one the binary's signature records. So at startup Photon makes its bundle's Info.plist byte-identical to the embedded one (macos/Info.plist, the complete bundle plist) and re-registers the bundle with Launch Services; macOS asks on the next request, or the one after a relaunch.
pub fn repair_bundle_plist() {
    #[cfg(target_os = "macos")]
    {
        const EMBEDDED: &str = include_str!("../../macos/Info.plist");
        let Ok(exe) = std::env::current_exe() else { return };
        // …/Photon Messenger.app/Contents/MacOS/photon-messenger
        let Some(contents) = exe.parent().and_then(|m| m.parent()) else { return };
        let Some(bundle) = contents.parent() else { return };
        if bundle.extension().and_then(|e| e.to_str()) != Some("app") {
            return; // the bare binary — its embedded plist is what macOS reads
        }
        let plist = contents.join("Info.plist");
        if std::fs::read_to_string(&plist).ok().as_deref() == Some(EMBEDDED) {
            return;
        }
        match std::fs::write(&plist, EMBEDDED) {
            Ok(()) => {
                let _ = std::process::Command::new("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister")
                    .arg("-f")
                    .arg(bundle)
                    .status();
                crate::logf!("macOS: the app bundle's Info.plist was stale (no microphone/camera description, and not the plist the signature records) — rewrote it from the embedded one and re-registered {}", bundle.display());
            }
            Err(e) => crate::logf!("macOS: could not rewrite the bundle's Info.plist ({}): {}", plist.display(), e),
        }
    }
}

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
