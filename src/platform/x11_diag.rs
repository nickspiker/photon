//! X11 CONNECTION DIAGNOSTICS (Nick 2026-10-31: the desktop window died five seconds after a reload — winit panicked "Failed to set cursor: Connection(IoError(UnknownError))", XCB's code for a socket the X server has closed — and nothing on record said why the server dropped us; "it does it often enough we'll see it again today").
//! Two records, so the next one names its cause:
//! - every X protocol error, logged as it arrives (winit reports them only thru `tracing`, which Photon does not collect — an error that preceded the drop was invisible);
//! - at a panic, the state of each of this process's sockets to the X server: closed by the server (an orderly close: the server dropped the client, for a protocol error or an XKillClient), reset, or still open.

use std::sync::atomic::{AtomicU64, Ordering};

/// X protocol errors seen this run.
static X_ERRORS: AtomicU64 = AtomicU64::new(0);
/// Every error is logged up to this many; past it, every LOG_EVERY-th, with the running count, so a stuck error loop cannot flood the log.
const LOG_FIRST: u64 = 1 << 6;
const LOG_EVERY: u64 = 1 << 10;

/// Log every X protocol error winit's Xlib connection receives. The hook returns false, so winit still handles each error as before.
pub fn install() {
    winit::platform::x11::register_xlib_error_hook(Box::new(|_display, event| {
        // SAFETY: winit calls the hook from its Xlib error handler with the XErrorEvent Xlib handed it, valid for the call.
        let e = unsafe { &*(event as *const x11::xlib::XErrorEvent) };
        let n = X_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
        if n <= LOG_FIRST || n % LOG_EVERY == 0 {
            crate::logf!(
                "X11: protocol error #{} — code {}, request {}.{}, resource {:#x}, serial {}",
                n,
                e.error_code,
                e.request_code,
                e.minor_code,
                e.resourceid,
                e.serial
            );
        }
        false
    }));
}

/// The state of every socket this process holds to an X server, one line each, read without consuming anything: `recv` with MSG_PEEK | MSG_DONTWAIT returns 0 when the server closed its end, would-block when the connection is open and idle, bytes when events wait unread, or the socket's error.
pub fn socket_report() -> String {
    use std::os::fd::FromRawFd;
    use std::os::linux::net::SocketAddrExt;
    let mut lines = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc/self/fd") else {
        return "X11: /proc/self/fd unreadable".into();
    };
    for entry in dir.flatten() {
        let Some(fd) = entry.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else {
            continue;
        };
        if !std::fs::read_link(entry.path()).is_ok_and(|t| t.to_string_lossy().starts_with("socket:")) {
            continue;
        }
        // Borrowed, never closed: the ManuallyDrop keeps the stream from closing a descriptor it does not own.
        let s = std::mem::ManuallyDrop::new(unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) });
        let Ok(peer) = s.peer_addr() else {
            continue;
        };
        let name = match (peer.as_pathname(), peer.as_abstract_name()) {
            (Some(p), _) => p.to_string_lossy().into_owned(),
            (None, Some(a)) => format!("@{}", String::from_utf8_lossy(a)),
            _ => continue,
        };
        if !name.contains(".X11-unix") {
            continue;
        }
        let mut b = [0u8; 1];
        // SAFETY: a one-byte peek into a stack buffer on a descriptor this process holds.
        let r = unsafe { libc::recv(fd, b.as_mut_ptr() as *mut libc::c_void, 1, libc::MSG_PEEK | libc::MSG_DONTWAIT) };
        let state = match r {
            0 => "CLOSED BY THE SERVER (orderly close: the server dropped this client)".to_string(),
            n if n > 0 => "open, events waiting unread".to_string(),
            _ => {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::WouldBlock { "open, idle".to_string() } else { format!("error: {err}") }
            }
        };
        let so_error = s.take_error().ok().flatten().map_or("none".to_string(), |e| e.to_string());
        lines.push(format!("X11: socket fd {fd} → {name}: {state}; SO_ERROR {so_error}"));
    }
    lines.push(format!("X11: {} protocol error(s) seen this run", X_ERRORS.load(Ordering::Relaxed)));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    /// A live connection to this desktop's X server reads as open (skipped where no X server is reachable — CI, a console).
    #[test]
    fn an_open_x_connection_reads_as_open() {
        // SAFETY: a plain Xlib open and close on this thread.
        let d = unsafe { x11::xlib::XOpenDisplay(std::ptr::null()) };
        if d.is_null() {
            return;
        }
        let report = super::socket_report();
        unsafe { x11::xlib::XCloseDisplay(d) };
        assert!(report.contains("open, idle") || report.contains("open, events waiting"), "{report}");
    }
}
