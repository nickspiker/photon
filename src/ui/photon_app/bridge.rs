//! BRIDGE remote terminal + unattended reboot — persistent per-sibling shell, bridge conversations, command execution, and the reboot-capsule/unattended markers.

use super::*;

/// Work item for the off-thread bridge executor: run a command in a sibling's persistent shell, or reset (kill) that sibling's shell so the next command starts fresh.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
pub(super) enum BridgeJob {
    Run(usize, [u8; 32], String),
    Reset([u8; 32]),
}

/// One persistent shell for a sibling's bridge session (host side). A single long-lived `bash` process — spawned on the first command, reused for every command after — so working directory, exported vars, and shell state persist exactly like a real terminal, instead of a fresh `bash -lc` per command (Nick, 2026-08-22). A reader thread drains stdout into a channel so a command that hangs (reads stdin, never returns) can't wedge the session forever: `run` waits with a timeout and the caller respawns on failure. stderr is merged into stdout in-shell (`exec 2>&1`); a per-session random sentinel marks each command's output boundary + exit code — shell-internal only, it NEVER touches the Photon wire.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
pub(super) struct BridgeShell {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    lines: std::sync::mpsc::Receiver<Option<String>>,
    sentinel: String,
}

#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
impl BridgeShell {
    /// Per-command wall-clock cap. A command still running after this returns an error; the caller drops the shell so the next command starts fresh — the anti-wedge for `cat`, a REPL, or a genuinely long job.
    const RUN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
    const MAX_OUT: usize = 12 * 1024;

    fn spawn() -> std::io::Result<BridgeShell> {
        use std::io::{BufRead, Write};
        use std::process::{Command, Stdio};
        let mut child = Command::new("bash")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()) // merged into stdout below via `exec 2>&1`
            .spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        // Reader thread: every stdout line becomes a channel item; EOF (shell died) sends one `None` and ends.
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = tx.send(None);
                        break;
                    }
                    Ok(_) => {
                        if tx.send(Some(line)).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(None);
                        break;
                    }
                }
            }
        });
        let sentinel = format!("__PHOTON_BRIDGE_{:016x}__", rand::random::<u64>());
        // Init: merge stderr→stdout, load aliases best-effort (guarded .bashrc often skips non-interactive, so force expand_aliases + source explicitly), silence any prompt. `true` gives the priming run below a clean exit to read up to.
        writeln!(
            stdin,
            "exec 2>&1; shopt -s expand_aliases 2>/dev/null; [ -f ~/.bashrc ] && source ~/.bashrc 2>/dev/null; [ -f ~/.bash_aliases ] && source ~/.bash_aliases 2>/dev/null; PS1=''; PROMPT_COMMAND=''; true"
        )?;
        let mut sh = BridgeShell { child, stdin, lines: rx, sentinel };
        // Drain init noise (bashrc chatter/errors) up to a priming sentinel so the FIRST real command's output starts clean.
        let _ = sh.run("true");
        Ok(sh)
    }

    /// Feed one command, then a sentinel that carries the command's exit code; collect every line until the sentinel. Returns the formatted output, or Err on shell death / timeout (caller respawns).
    fn run(&mut self, cmd: &str) -> Result<String, String> {
        use std::io::Write;
        writeln!(self.stdin, "{}", cmd).map_err(|e| e.to_string())?;
        // `$?` here is `cmd`'s exit — expanded on this line, right after cmd ran.
        writeln!(self.stdin, "printf '%s %d\\n' '{}' \"$?\"", self.sentinel)
            .map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + Self::RUN_TIMEOUT;
        let mut body = String::new();
        let mut code = 0i32;
        loop {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or_else(|| "command timed out".to_string())?;
            match self.lines.recv_timeout(remaining) {
                Ok(Some(line)) => {
                    if let Some(rest) = line.trim_end().strip_prefix(&self.sentinel) {
                        code = rest.trim().parse().unwrap_or(-1);
                        break;
                    }
                    if body.len() < Self::MAX_OUT {
                        body.push_str(&line);
                    }
                }
                Ok(None) => return Err("shell exited".to_string()),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err("command timed out".to_string())
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("shell reader gone".to_string())
                }
            }
        }
        if body.len() >= Self::MAX_OUT {
            body.push_str("\n\u{2026}(output truncated)");
        }
        let trimmed = body.trim_end_matches('\n');
        let out = if trimmed.is_empty() {
            if code == 0 {
                "(no output)".to_string()
            } else {
                format!("(no output, exit {})", code)
            }
        } else if code != 0 {
            format!("{}\n[exit {}]", trimmed, code)
        } else {
            trimmed.to_string()
        };
        Ok(out)
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl PhotonApp {
    /// Enter the add-device (pairing-words) flow. Was the interim Ready-orb action; now reached from the Fleet page's "Add device" pill. Spawns the bindreq watch so the candidate set is live before the first keystroke.
    /// Open a command conversation with a specific sibling DEVICE (the Bridge button). Siblings aren't listed as ordinary conversations, but they ARE contacts — find the one carrying this device pubkey and open it, so the chat-as-shell path (`$ cmd`) has a per-device surface. A seed hint message is inserted the first time so the screen isn't blank.
    pub(super) fn open_bridge_conversation(&mut self, device: [u8; 32]) {
        let idx = self
            .contacts
            .iter()
            .position(|c| c.is_sibling && c.public_identity.key == device);
        let Some(ci) = idx else {
            self.ready_toast = Some("That device isn't paired as a sibling yet.".to_string());
            self.ready_toast_screen = None;
            crate::log("BRIDGE: no sibling contact for that device — cannot open");
            return;
        };
        // OPEN = FRESH SESSION (Nick 2026-08-22): wipe the screen (bridge rows are ephemeral RAM anyway). No hint blurb — a terminal doesn't explain itself; it's a clean screen until output arrives.
        if let Some(conv) = self.conv_mut_of(ci) {
            conv.messages.clear();
        }
        // Tell the host to kill its shell for us so the FIRST command starts a genuinely fresh session (cwd/env reset), matching the wiped screen.
        #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
        self.send_bridge_reset(ci);
        self.open_conversation_with(ci);
        self.state = AppState::Conversation;
        self.conv_topbar_off = 0.0;
        self.clear_unread(ci);
        self.change_focus(None);
        crate::logf!(
            "BRIDGE: opened command conversation with sibling {}",
            crate::fp(&device)
        );
    }

    /// Fire an attest with caller-supplied roots (the probe already derived them), skipping the permanence interstitial and the second proof. First-attest persistence semantics.
    /// Device-scope vault entry holding the unattended reboot capsule (was the `<config>/reboot_capsule` loose file). The device vault opens pre-attest, so the boot path reads it before any UI.
    pub(super) const REBOOT_CAPSULE_ENTRY: &'static str = "capsule/reboot";

    /// Whether unattended auto-attest-on-reboot is enabled (default OFF — flag absent). Device-scope vault flag (was the `<config>/unattended_reboot` marker file).
    pub(super) fn unattended_enabled() -> bool {
        crate::storage::device_flag("flags/unattended_reboot")
    }

    /// HOST role, chat transport: a NEW command arrived as an ordinary chat message in the sibling `ci`'s conversation — dispatch it to the OFF-THREAD bridge executor, which runs it in that sibling's PERSISTENT shell and posts the raw output back for `drain_bridge_output` to reply with (typed RefKind::BridgeOut so it renders but never re-runs). Running the shell inline froze the host's event loop for the command's whole duration, stalling the ACK it owes the operator (field 2026-08-22). ONE shell per sibling, spawned on first command and reused after so `cd`/env/state persist like a real session; the executor thread OWNS the shells so nothing blocks the UI.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn run_bridge_command_chat(&mut self, ci: usize, cmd: &str) {
        let Some(dev) = self.contacts.get(ci).map(|c| c.public_identity.key) else {
            return;
        };
        self.ensure_bridge_exec();
        if let Some(tx) = self.bridge_cmd_tx.as_ref() {
            let _ = tx.send(BridgeJob::Run(ci, dev, cmd.to_string()));
        }
    }

    /// Client OPENED the bridge → tell the host to drop its shell for us so the next command starts a FRESH session (Nick 2026-08-22). Fire-and-forget over the chain as a hidden BridgeReset control row.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn send_bridge_reset(&mut self, ci: usize) {
        // A minimal payload; the TYPED reference carries the meaning and the row is hidden either way.
        self.send_chain_message(ci, " ", false, Some((crate::types::RefKind::BridgeReset, 0)));
    }

    /// Host received a BridgeReset (the peer opened the bridge) → drop that sibling's shell so the next command respawns fresh.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn reset_bridge_shell(&mut self, ci: usize) {
        let Some(dev) = self.contacts.get(ci).map(|c| c.public_identity.key) else {
            return;
        };
        self.ensure_bridge_exec();
        if let Some(tx) = self.bridge_cmd_tx.as_ref() {
            let _ = tx.send(BridgeJob::Reset(dev));
        }
    }

    /// Lazily spawn the off-thread bridge executor (owns one persistent shell per sibling device). No-op once running.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    fn ensure_bridge_exec(&mut self) {
        if self.bridge_cmd_tx.is_some() {
            return;
        }
        let wake = self.event_proxy.clone();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<BridgeJob>();
        let (out_tx, out_rx) = std::sync::mpsc::channel::<(usize, String)>();
        self.bridge_out_rx = Some(out_rx);
        std::thread::Builder::new()
            .name("bridge-exec".to_string())
            .spawn(move || {
                let mut shells: std::collections::HashMap<[u8; 32], BridgeShell> =
                    std::collections::HashMap::new();
                while let Ok(job) = cmd_rx.recv() {
                    let (ci, dev, cmd) = match job {
                        BridgeJob::Run(ci, dev, cmd) => (ci, dev, cmd),
                        BridgeJob::Reset(dev) => {
                            // Kill the old shell; the next Run respawns a fresh session.
                            if let Some(mut sh) = shells.remove(&dev) {
                                sh.kill();
                            }
                            continue;
                        }
                    };
                    crate::logf!("BRIDGE: running command from sibling: {}", cmd);
                    if !shells.contains_key(&dev) {
                        match BridgeShell::spawn() {
                            Ok(sh) => {
                                shells.insert(dev, sh);
                            }
                            Err(e) => {
                                let _ = out_tx.send((ci, format!("(bridge shell failed to start: {e})")));
                                if let Some(w) = wake.as_ref() {
                                    let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                                }
                                continue;
                            }
                        }
                    }
                    let out = match shells.get_mut(&dev).unwrap().run(&cmd) {
                        Ok(o) => o,
                        Err(e) => {
                            // Wedged/died (a command that read stdin, or the shell exited) — drop it so the NEXT command respawns a fresh session.
                            shells.remove(&dev);
                            format!("(session reset: {e})")
                        }
                    };
                    let _ = out_tx.send((ci, out));
                    if let Some(w) = wake.as_ref() {
                        let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                    }
                }
            })
            .expect("spawn bridge-exec");
        self.bridge_cmd_tx = Some(cmd_tx);
    }

    /// Tick drain: reply with each finished command's output over the durable chain (RefKind::BridgeOut). UI thread, but zero shell work — just the send. No-op when the executor isn't running.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn drain_bridge_output(&mut self) {
        let outs: Vec<(usize, String)> = match self.bridge_out_rx.as_ref() {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        for (ci, out) in outs {
            self.send_chain_message(ci, &out, false, Some((crate::types::RefKind::BridgeOut, 0)));
        }
    }

    /// Turn unattended mode on/off. ON writes the marker AND (also requires background/autostart so a reboot actually relaunches photon) refreshes the capsule from the live session. OFF removes the marker and shreds the capsule.
    pub(super) fn set_unattended(&mut self, on: bool) {
        crate::storage::set_device_flag("flags/unattended_reboot", on);
        if on {
            // Unattended only means anything if the box relaunches photon at boot — force background/autostart on.
            #[cfg(not(target_os = "android"))]
            {
                let _ = crate::platform::autostart::enable();
                crate::platform::autostart::set_background_desired(true);
                self.resident_mode = true;
            }
            self.refresh_reboot_capsule();
            crate::log(
                "UNATTENDED: auto-attest-on-reboot ENABLED (also forced background/autostart on)",
            );
        } else {
            if let Some(v) = crate::storage::device_vault() {
                let _ = v.delete_device(Self::REBOOT_CAPSULE_ENTRY);
            }
            crate::log("UNATTENDED: auto-attest-on-reboot DISABLED (capsule shredded)");
        }
    }

    /// Refresh the reboot capsule from the live session IFF unattended mode is on; otherwise ensure no capsule exists. Called on every successful attest and on toggle-on.
    pub(super) fn refresh_reboot_capsule(&self) {
        let Some(vault) = crate::storage::device_vault() else {
            return;
        };
        if Self::unattended_enabled() {
            if let Some(session) = self.session.as_ref() {
                let stored = tohu::seal_reboot_capsule(session)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| vault.write_device(Self::REBOOT_CAPSULE_ENTRY, &bytes).map_err(|e| e.to_string()));
                match stored {
                    Ok(()) => crate::log("UNATTENDED: reboot capsule refreshed (device-bound; opens only on this hardware)"),
                    Err(e) => crate::logf!("UNATTENDED: reboot capsule write failed: {}", e),
                }
            }
        } else {
            let _ = vault.delete_device(Self::REBOOT_CAPSULE_ENTRY);
        }
    }
}
