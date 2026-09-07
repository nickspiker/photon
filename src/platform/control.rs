//! Second-launch handoff — the control channel a fresh `photon-messenger` invocation uses to tell the RESIDENT instance "show yourself" instead of dying with an "already running" error.
//! One verb, one direction: `show\n`. No state crosses; the resident side reacts by surfacing its window (`PhotonEvent::ShowWindow` → `EventResponse::ShowWindow`). Anything richer belongs in the app protocol, not here.
//! Transport per platform: a Unix domain socket at the dir-keyed runtime path (`storage::runtime_artifact`; created ONLY after the flock single-instance guard is won, so a stale path can be unlinked safely), and on Windows the single-instance TcpListener itself doubles as the channel (it already exists, it's already dir-keyed, and any same-user process that could connect could equally just launch the app — "show the window" needs no authentication).

use std::io::{Read, Write};

/// The listener the resident instance accepts handoffs on, parked here by `main` (which owns lock acquisition) until the app spawns the accept thread once it has an event proxy to forward to.
enum ControlListener {
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixListener),
    Tcp(std::net::TcpListener),
}

static LISTENER: std::sync::Mutex<Option<ControlListener>> = std::sync::Mutex::new(None);

#[cfg(unix)]
fn socket_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    crate::storage::runtime_artifact(data_dir, "sock")
}

/// Resident side, unix: bind the control socket. Call ONLY while holding the single-instance flock — that guarantee is what makes unlinking a leftover socket path safe (the previous owner is dead; the kernel freed its flock).
#[cfg(unix)]
pub fn install_unix_listener(data_dir: &std::path::Path) {
    let path = socket_path(data_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(&path);
    match std::os::unix::net::UnixListener::bind(&path) {
        Ok(l) => *LISTENER.lock().unwrap() = Some(ControlListener::Unix(l)),
        Err(e) => crate::logf!(
            "CONTROL: bind {} failed: {} (second-launch handoff disabled)",
            path.display(),
            e
        ),
    }
}

/// Resident side, windows: adopt a clone of the single-instance TcpListener as the control channel.
pub fn install_tcp_listener(listener: std::net::TcpListener) {
    *LISTENER.lock().unwrap() = Some(ControlListener::Tcp(listener));
}

/// Resident side: start the accept loop, forwarding each `show` to the UI thread via the wake proxy. Called once from `set_event_proxy`; a no-op if `main` never parked a listener (handoff disabled, nothing to serve).
/// The CURRENT wake target — swappable so a re-registered proxy (the embedded lifeline replacing a dead winit loop's, docs/headless-lifeline.md) re-aims the long-lived accept thread instead of it poking a corpse.
static CURRENT_PROXY: std::sync::Mutex<
    Option<std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>>,
> = std::sync::Mutex::new(None);

pub fn spawn_accept_thread(
    proxy: std::sync::Arc<dyn fluor::host::WakeSender<crate::ui::PhotonEvent>>,
) {
    // Refresh FIRST: on a second registration the listener is already taken and we return below, but the running thread must still start waking the NEW loop.
    *CURRENT_PROXY.lock().unwrap() = Some(proxy);
    let Some(listener) = LISTENER.lock().unwrap().take() else {
        return;
    };
    std::thread::spawn(move || {
        let handle = |buf: &[u8]| {
            let Some(proxy) = CURRENT_PROXY.lock().unwrap().clone() else {
                return;
            };
            let proxy = &proxy;
            if buf.starts_with(b"show") {
                crate::log("CONTROL: show requested by a second launch — surfacing the window");
                let _ = proxy.send(crate::ui::PhotonEvent::ShowWindow);
            } else if buf.starts_with(b"yield") {
                // A full-UI launch wants the lock (docs/headless-lifeline.md). The LIFELINE pump exits on this event; a full-UI instance just surfaces (its on_user_event maps Yield → ShowWindow behavior) and keeps the lock — the requester's re-acquire then fails and it falls back to the show path.
                crate::log("CONTROL: yield requested by a full-UI launch");
                let _ = proxy.send(crate::ui::PhotonEvent::Yield);
            }
        };
        match listener {
            #[cfg(unix)]
            ControlListener::Unix(l) => {
                for stream in l.incoming() {
                    let Ok(mut s) = stream else { continue };
                    let mut buf = [0u8; 16];
                    let n = s.read(&mut buf).unwrap_or(0);
                    handle(&buf[..n]);
                }
            }
            ControlListener::Tcp(l) => {
                for stream in l.incoming() {
                    let Ok(mut s) = stream else { continue };
                    // Loopback-only by bind; a brief read deadline so a port-scanner's half-open connect can't wedge the accept loop.
                    let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(500)));
                    let mut buf = [0u8; 16];
                    let n = s.read(&mut buf).unwrap_or(0);
                    handle(&buf[..n]);
                }
            }
        }
    });
}

/// Full-UI-launch side of the lifeline handoff: ask whoever holds the lock to yield it. `true` = delivered; the caller then retries the lock (a lifeline exits and frees it; a full-UI resident keeps it and surfaces instead).
pub fn request_yield(data_dir: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        let path = socket_path(data_dir);
        if let Ok(mut s) = std::os::unix::net::UnixStream::connect(&path) {
            return s.write_all(b"yield\n").is_ok();
        }
        false
    }
    #[cfg(not(unix))]
    {
        let h = blake3::hash(data_dir.to_string_lossy().as_bytes());
        let port = 20000 + (u16::from_le_bytes([h.as_bytes()[0], h.as_bytes()[1]]) % 20000);
        if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            return s.write_all(b"yield\n").is_ok();
        }
        false
    }
}

/// Second-launch side: ask the resident instance to surface. `true` = delivered (the caller should exit 0 quietly); `false` = nobody answered (stale lock? different failure — caller falls back to the old already-running error).
pub fn request_show(data_dir: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        let path = socket_path(data_dir);
        if let Ok(mut s) = std::os::unix::net::UnixStream::connect(&path) {
            return s.write_all(b"show\n").is_ok();
        }
        false
    }
    #[cfg(not(unix))]
    {
        // Mirror of storage::acquire_single_instance's dir-keyed port derivation.
        let h = blake3::hash(data_dir.to_string_lossy().as_bytes());
        let port = 20000 + (u16::from_le_bytes([h.as_bytes()[0], h.as_bytes()[1]]) % 20000);
        if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            return s.write_all(b"show\n").is_ok();
        }
        false
    }
}
