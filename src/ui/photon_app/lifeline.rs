//! The headless lifeline (docs/headless-lifeline.md): run photon's display-free core — vault, capsule auto-attest, network, bridge — with no fluor, no winit, no display. The remote lifeline may depend on the network and the vault, never on a display server (the 2026-09-04 three-day black-greeter incident).

use super::*;

/// Headless WakeSender: network threads poke this instead of a winit EventLoopProxy; the pump loop wakes on the condvar edge or its idle timeout.
struct HeadlessWaker {
    queue: std::sync::Mutex<std::collections::VecDeque<PhotonEvent>>,
    cv: std::sync::Condvar,
}

impl fluor::host::WakeSender<PhotonEvent> for HeadlessWaker {
    fn send(&self, event: PhotonEvent) -> Result<(), fluor::host::WakeError> {
        self.queue.lock().unwrap().push_back(event);
        self.cv.notify_one();
        Ok(())
    }
}

/// The idle tick cadence: wake edges (network events thru the waker) drive the real work; this is only the retransmit/presence heartbeat floor, matching the UI loop's idle order of magnitude.
const LIFELINE_TICK: std::time::Duration = std::time::Duration::from_millis(250);

/// Run the lifeline until a full-UI launch asks for the lock (Yield) — the caller then returns from main, releasing the single-instance lock for the full instance to take.
pub fn run_lifeline(mut app: PhotonApp) {
    let waker = std::sync::Arc::new(HeadlessWaker { queue: Default::default(), cv: std::sync::Condvar::new() });
    // set_event_proxy spawns the control accept thread (the yield channel) exactly as the host would; resident-mode tray stays off (the field defaults false and nothing headless flips it).
    fluor::host::app::FluorApp::set_event_proxy(&mut app, waker.clone());
    // The identical display-free startup the UI runs: network stack, job channels, unattended capsule → auto-attest, vault open + session resume. No capsule = the pump idles pre-attest and serves nothing sensitive — lifeline is exactly as available as unattended mode.
    // EMBEDDED takeover (Tier 2: the event loop died under a living app) arrives already initialized — re-running core_init would double-spawn the network stack; the pump alone is the whole job then.
    if app.handle_query.is_none() {
        app.core_init();
    } else {
        crate::log("LIFELINE: embedded takeover — core already live, pump only (same session, zero attest gap)");
    }
    crate::logf!(
        "LIFELINE: headless pump up — attested: {}, waiting on network edges (Yield hands off to a full-UI launch)",
        tohu::session().is_some()
    );
    loop {
        // Drain the wake queue first: Yield ends the lifeline; every other variant is a pure wake (the pump below drains whatever channel the sender filled — same contract as on_user_event).
        {
            let mut q = waker.queue.lock().unwrap();
            loop {
                match q.pop_front() {
                    Some(PhotonEvent::Yield) => {
                        crate::log("LIFELINE: yield requested — releasing the instance lock to the full-UI launch");
                        return;
                    }
                    Some(_) => continue,
                    None => break,
                }
            }
            // Idle wait: a wake edge or the heartbeat floor, whichever first. The guard drops before the pump runs.
            let _ = waker.cv.wait_timeout(q, LIFELINE_TICK);
        }
        // THE pump — advance_protocol wraps the full status drain (check_status_updates) plus pings, retransmits, chain sweeps, and the bridge drains. Ctx-free by construction (the core_init split's compiler-proven contract).
        let _ = app.advance_protocol(std::time::Instant::now());
    }
}
