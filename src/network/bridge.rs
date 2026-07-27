//! The BRIDGE — a passless remote shell between fleet siblings, carried over Photon Transport instead of SSH.
//!
//! A sibling device (the client) opens a real PTY on another of the operator's devices (the host); keystrokes and terminal
//! output ride `term` frames (see [`crate::network::fgtw::protocol::build_term_vsf`]) sealed under the FLEET key. There is
//! no password because possession of a fold-verified sibling device IS the credential — the same trust the fleet key already
//! encodes. It is OFF by default and gated behind a Security toggle on the host; a session opening fires a notification there.
//!
//! This module is the HOST half: it owns the live PTY sessions (one child shell each), pumps the shell's output back to the
//! client, and applies keystrokes / resizes / kill-and-respawn. The CLIENT half is the `photonsh` binary, which proxies raw
//! stdin/stdout through the resident photon over a local unix socket. Desktop-only (real PTY); compiled out on Android.

#![cfg(all(unix, not(target_os = "android")))]

use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::sync::mpsc::Sender;

/// The blake3 KDF context binding a `term` payload seal to this feature — folded with the fleet key so a term payload can't be
/// confused with any other fleet-key-sealed blob.
fn term_seal_key(fleet_key: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key("photon.bridge.term.v0", fleet_key)
}

/// Seal a term payload (keystrokes / output / control args) under the fleet key.
pub fn seal_term(payload: &[u8], fleet_key: &[u8; 32]) -> Result<Vec<u8>, String> {
    kete::encrypt_bytes(payload, &term_seal_key(fleet_key))
}

/// Open a term payload sealed by [`seal_term`]. `None` on wrong key / tamper.
pub fn open_term(sealed: &[u8], fleet_key: &[u8; 32]) -> Option<Vec<u8>> {
    kete::decrypt_bytes(sealed, &term_seal_key(fleet_key)).ok()
}

/// One live PTY session on the host: the master fd (we read shell output from it and write keystrokes to it) and the child pid
/// (killed on close / nuke). The output-reader thread owns a dup of the master fd and streams `TermOut` events until EOF.
struct PtySession {
    master_fd: RawFd,
    child_pid: libc::pid_t,
    /// Bumped on NUKE so the stale reader thread's events are dropped once a fresh shell takes the slot.
    generation: u64,
}

/// An event from a host PTY reader thread back to the app (which seals + sends it as a `term` DATA/EXIT frame to the client).
pub enum TermOut {
    /// Shell produced output for `session_id`.
    Data { session_id: [u8; 16], generation: u64, bytes: Vec<u8> },
    /// Shell for `session_id` exited with `code`.
    Exit { session_id: [u8; 16], generation: u64, code: i32 },
}

/// The host-side registry of live sessions. Lives in the app; every method is cheap and non-blocking (the blocking reads happen
/// on per-session threads that post `TermOut` back through the channel handed to [`Self::open`]).
#[derive(Default)]
pub struct BridgeHost {
    sessions: HashMap<[u8; 16], PtySession>,
    next_generation: u64,
}

impl BridgeHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a session is live.
    pub fn has(&self, session_id: &[u8; 16]) -> bool {
        self.sessions.contains_key(session_id)
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Open a fresh shell for `session_id` at `cols`×`rows`. Spawns the child via `forkpty` and a reader thread that streams the
    /// shell's output back through `out_tx` until EOF, then posts `Exit`. Idempotent-ish: an existing session for the same id is
    /// killed first (a re-OPEN = reconnect intent). Returns the reader-thread's generation, or an error string.
    pub fn open(
        &mut self,
        session_id: [u8; 16],
        cols: u16,
        rows: u16,
        out_tx: Sender<TermOut>,
    ) -> Result<u64, String> {
        if self.sessions.contains_key(&session_id) {
            self.close(&session_id);
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1);

        let (master_fd, child_pid) = spawn_pty_shell(cols, rows)?;
        self.sessions.insert(session_id, PtySession { master_fd, child_pid, generation });

        // Reader thread: dup the master so the thread owns its own fd lifetime, then blocking-read until EOF.
        let read_fd = unsafe { libc::dup(master_fd) };
        if read_fd < 0 {
            self.close(&session_id);
            return Err("dup(master) failed".to_string());
        }
        let pid = child_pid;
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                let n = unsafe { libc::read(read_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                if n > 0 {
                    let _ = out_tx.send(TermOut::Data {
                        session_id,
                        generation,
                        bytes: buf[..n as usize].to_vec(),
                    });
                } else {
                    // n == 0 (EOF: shell exited, PTY closed) or n < 0 (error). Reap the child for its exit code.
                    break;
                }
            }
            let mut status: libc::c_int = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };
            let code = if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { -1 };
            unsafe { libc::close(read_fd) };
            let _ = out_tx.send(TermOut::Exit { session_id, generation, code });
        });

        Ok(generation)
    }

    /// Feed keystrokes to the shell.
    pub fn write_input(&mut self, session_id: &[u8; 16], bytes: &[u8]) {
        if let Some(s) = self.sessions.get(session_id) {
            let mut off = 0;
            while off < bytes.len() {
                let n = unsafe {
                    libc::write(
                        s.master_fd,
                        bytes[off..].as_ptr() as *const libc::c_void,
                        bytes.len() - off,
                    )
                };
                if n <= 0 {
                    break;
                }
                off += n as usize;
            }
        }
    }

    /// Resize the shell's window.
    pub fn resize(&mut self, session_id: &[u8; 16], cols: u16, rows: u16) {
        if let Some(s) = self.sessions.get(session_id) {
            set_winsize(s.master_fd, cols, rows);
        }
    }

    /// Kill + close a session (client CLOSE, or making room for a re-OPEN). Best-effort SIGKILL then fd close.
    pub fn close(&mut self, session_id: &[u8; 16]) {
        if let Some(s) = self.sessions.remove(session_id) {
            unsafe {
                libc::kill(s.child_pid, libc::SIGKILL);
                libc::close(s.master_fd);
            }
        }
    }

    /// The "nuke a hung bash" button: kill the current shell and spawn a fresh one on the SAME session_id, so the client keeps
    /// its session without re-opening. Returns the new generation.
    pub fn nuke(
        &mut self,
        session_id: [u8; 16],
        cols: u16,
        rows: u16,
        out_tx: Sender<TermOut>,
    ) -> Result<u64, String> {
        self.close(&session_id);
        self.open(session_id, cols, rows, out_tx)
    }

    /// The generation currently owning `session_id` (an event tagged with a stale generation after a nuke is dropped).
    pub fn generation(&self, session_id: &[u8; 16]) -> Option<u64> {
        self.sessions.get(session_id).map(|s| s.generation)
    }

    /// Tear down every session (app shutdown / toggle-off).
    pub fn close_all(&mut self) {
        let ids: Vec<[u8; 16]> = self.sessions.keys().copied().collect();
        for id in ids {
            self.close(&id);
        }
    }
}

impl Drop for BridgeHost {
    fn drop(&mut self) {
        self.close_all();
    }
}

/// `cols<<16 | rows` packing used in OPEN/RESIZE payloads (4 bytes BE).
pub fn pack_winsize(cols: u16, rows: u16) -> [u8; 4] {
    let v = ((cols as u32) << 16) | rows as u32;
    v.to_be_bytes()
}

/// Unpack a 4-byte BE winsize payload → (cols, rows). Defaults 80×24 on a short/absent payload.
pub fn unpack_winsize(payload: &[u8]) -> (u16, u16) {
    if payload.len() >= 4 {
        let v = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
        (((v >> 16) & 0xFFFF) as u16, (v & 0xFFFF) as u16)
    } else {
        (80, 24)
    }
}

/// `forkpty` + exec the user's login shell in the child. Returns (master_fd, child_pid). The child never returns (it execs or
/// _exits); only the parent path returns Ok.
fn spawn_pty_shell(cols: u16, rows: u16) -> Result<(RawFd, libc::pid_t), String> {
    let mut master: libc::c_int = 0;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    ws.ws_col = cols;
    ws.ws_row = rows;

    let pid = unsafe {
        libc::forkpty(
            &mut master,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut ws,
        )
    };
    if pid < 0 {
        return Err("forkpty failed".to_string());
    }
    if pid == 0 {
        // CHILD: exec the login shell. Set TERM so full-screen apps (vim, htop) work; inherit the rest of the environment.
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let shell_c = std::ffi::CString::new(shell).unwrap_or_else(|_| std::ffi::CString::new("/bin/bash").unwrap());
        unsafe {
            // A fresh session so the shell is a real controlling-terminal leader.
            libc::setsid();
            let term = std::ffi::CString::new("TERM=xterm-256color").unwrap();
            libc::putenv(term.into_raw());
            let argv = [shell_c.as_ptr(), std::ptr::null()];
            libc::execv(shell_c.as_ptr(), argv.as_ptr());
            // execv only returns on failure.
            libc::_exit(127);
        }
    }
    // PARENT.
    Ok((master, pid))
}

/// TIOCSWINSZ on the master so the child sees a resize (SIGWINCH).
fn set_winsize(master_fd: RawFd, cols: u16, rows: u16) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    ws.ws_col = cols;
    ws.ws_row = rows;
    unsafe {
        libc::ioctl(master_fd, libc::TIOCSWINSZ, &ws);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winsize_pack_round_trips() {
        assert_eq!(unpack_winsize(&pack_winsize(120, 40)), (120, 40));
        assert_eq!(unpack_winsize(&pack_winsize(80, 24)), (80, 24));
        assert_eq!(unpack_winsize(&[]), (80, 24)); // default on empty
    }

    #[test]
    fn term_seal_round_trips_and_rejects_wrong_key() {
        let k1 = [7u8; 32];
        let k2 = [9u8; 32];
        let sealed = seal_term(b"echo hi\n", &k1).unwrap();
        assert_eq!(open_term(&sealed, &k1).as_deref(), Some(&b"echo hi\n"[..]));
        assert!(open_term(&sealed, &k2).is_none());
    }
}
