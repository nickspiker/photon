//! Hold the machine awake while a piece of work is genuinely in flight — and only then.
//!
//! A CLUTCH exchange is ~570KB across several round trips, and the relay has no mailbox: if the lid closes mid-ceremony the frames in flight are discarded, the peer sees silence, and the exchange half-completes in the way that leaves each side holding a different half. That is not hypothetical — it is the shape of the stalls in the field.
//!
//! Deliberately NOT a "keep Photon awake" mode. The assertion is scoped to a `SleepGuard` that releases on drop, so it can only ever last as long as the operation that took it. An app that holds one indefinitely is a battery bug wearing a feature's clothes.
//!
//! macOS only. Android's foreground service already keeps the process scheduled, and Linux/Windows desktops are not where this failure shows up (the MacBook is the machine that gets shut mid-conversation).

/// A held wake assertion. Dropping it lets the machine sleep again — so bind it to the work, never to the app.
pub struct SleepGuard {
    #[cfg(target_os = "macos")]
    id: u32,
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_void;

    // IOKit's power-management assertions. `PreventUserIdleSystemSleep` stops the IDLE timer from putting the system to sleep; it does NOT fight an explicit lid-close or a user-chosen Sleep, which is correct — overriding a deliberate human action is not ours to do.
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: *const c_void,
            assertion_level: u32,
            assertion_name: *const c_void,
            assertion_id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(assertion_id: u32) -> i32;
    }

    const LEVEL_ON: u32 = 255;

    /// Take the assertion. `None` if IOKit refuses, which is not an error worth surfacing — the work proceeds, it just is not protected from an idle sleep.
    pub fn hold(reason: &str) -> Option<u32> {
        use objc2_foundation::NSString;
        // NSString is toll-free bridged to CFStringRef, so these pointers are what IOKit expects.
        let kind = NSString::from_str("PreventUserIdleSystemSleep");
        let name = NSString::from_str(reason);
        let mut id: u32 = 0;
        let rc = unsafe {
            IOPMAssertionCreateWithName(
                objc2::rc::Retained::as_ptr(&kind) as *const c_void,
                LEVEL_ON,
                objc2::rc::Retained::as_ptr(&name) as *const c_void,
                &mut id,
            )
        };
        (rc == 0).then_some(id)
    }

    pub fn release(id: u32) {
        unsafe {
            IOPMAssertionRelease(id);
        }
    }
}

impl SleepGuard {
    /// Hold the machine awake for the duration of one operation. `reason` is what macOS shows in `pmset -g assertions`, so name the WORK ("photon: CLUTCH ceremony"), never the app — a user auditing why their laptop stayed up deserves a specific answer.
    #[cfg(target_os = "macos")]
    pub fn hold(reason: &str) -> Option<Self> {
        let id = imp::hold(reason)?;
        crate::logf!("AWAKE: holding off idle sleep — {}", reason);
        Some(Self { id })
    }

    /// Every other platform: the guard exists so call sites read the same, and does nothing. Android's foreground service already keeps the process alive.
    #[cfg(not(target_os = "macos"))]
    pub fn hold(_reason: &str) -> Option<Self> {
        None
    }
}

#[cfg(target_os = "macos")]
impl Drop for SleepGuard {
    fn drop(&mut self) {
        imp::release(self.id);
        crate::log("AWAKE: released — idle sleep allowed again");
    }
}
