//! BRIDGE remote terminal + unattended reboot — persistent per-sibling shell, bridge conversations, command execution, and the reboot-capsule/unattended markers.

use super::*;

/// Work item for the off-thread bridge executor: run a command in a sibling's persistent shell, or reset (kill) that sibling's shell so the next command starts fresh.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
pub(super) enum BridgeJob {
    /// (contact id, device, command text, the command row's eagle_time — the target streamed output frames reference).
    Run(ContactId, [u8; 32], String, i64),
    Reset([u8; 32]),
}

/// One streamed emission from the executor toward the wire: `body` is this emission's output bytes (a DELTA since 1358caa8 — the snapshot form this doc once described is gone; docs/bridge.md carries the re-serve rules), `target` is the command row's eagle_time (what the client's replace-in-place keys on), `fin` carries the exit code once the command completed, and the locus names where the shell stands so the operator is never blind to host+cwd again (field 2026-08-23: a pull meant for photon ran in keys/). Partials ride a latest-wins slot (a superseded snapshot is garbage by definition); finals ride the ordered channel because every one must reach the wire.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
pub(super) struct BridgeEmit {
    /// The sibling conversation, by id — the emit crosses the executor thread and back, and an index would drift after a removal.
    pub contact: ContactId,
    pub target: i64,
    pub seq: u64,
    /// The UNSENT output accumulated since the last frame that made it onto the wire — a DELTA, not a snapshot (Nick 2026-09-03: "just send what's missing"). The chain's hash links carry the ordering; the client appends.
    pub body: String,
    /// Bytes trimmed off this buffer's FRONT to hold the memory bound — named in the frame's elision marker so a gap is never silent.
    pub dropped: usize,
    pub fin: Option<i32>,
    pub host: String,
    pub cwd: String,
}

/// The interrupt registry the UI thread signals thru while a worker is blocked draining output: device → the shell's bash pid. The in-flight command is found live as bash's child TREE (a foreground group needs no announce — see run_streaming's foreground rationale). Written by workers at spawn/death, removed by Reset — its ABSENCE after a shell death tells the worker the death was a deliberate reset (swallow the "(shell died)" frame instead of sending it into a freshly wiped screen).
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
pub(super) type BridgeFgMap =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<[u8; 32], i32>>>;
/// Each sibling's shell CWD as of its last completed command — the bridge pigeon's landing directory ("show up in whatever folder the bridge is currently in", Nick 2026-09-17). The worker owns the shell and learns the cwd from the command sentinel; the UI thread reads it at landing time, so the map is the one seam between them. Absent or empty (no command run yet) = the shell's starting directory, which is home.
pub(super) type BridgeCwdMap =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<[u8; 32], String>>>;

/// Every live descendant of `root`, breadth-first via `pgrep -P` (present on every unix host the bridge ships to). The foreground command and everything it spawned — bash itself excluded by construction.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
fn bridge_child_tree(root: i32) -> Vec<i32> {
    let mut all: Vec<i32> = Vec::new();
    let mut frontier = vec![root];
    while let Some(p) = frontier.pop() {
        if let Ok(out) = std::process::Command::new("pgrep")
            .arg("-P")
            .arg(p.to_string())
            .output()
        {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Ok(c) = line.trim().parse::<i32>() {
                    all.push(c);
                    frontier.push(c);
                }
            }
        }
    }
    all
}

/// Unsent-delta buffer bound per command (Nick's delta redesign 2026-09-03): frames carry only what's NEW, so nothing is ever re-sent — the only cap left is host memory while a client is slow/unreachable. Past this, the buffer's FRONT is trimmed and the dropped byte count rides the next frame's elision marker (a gap is explicit, never silent). 64KB ≈ minutes of full-tilt cargo spew.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
const BRIDGE_BUF_MAX: usize = 65536;

/// Wake the event loop from a worker thread. Platform-agnostic (a plain event send) — the pigeon landing on any host uses it, so it carries no shell cfg gate.
pub(super) fn bridge_wake(w: &Option<std::sync::Arc<dyn WakeSender<PhotonEvent>>>) {
    if let Some(w) = w {
        let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
    }
}

/// One worker thread per sibling device, owning that device's persistent shell. Per-device because the executor used to serialize EVERY sibling thru one thread — with no timeout, one long build would have queued every other bridge behind it. The worker blocks in run_streaming for as long as the command takes; liveness is visible thru the streamed partials, and the operator's stop lever runs thru BridgeFgMap, not thru this queue.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
fn spawn_bridge_worker(
    dev: [u8; 32],
    partials: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<ContactId, BridgeEmit>>>,
    fg: BridgeFgMap,
    cwds: BridgeCwdMap,
    wake: Option<std::sync::Arc<dyn WakeSender<PhotonEvent>>>,
) -> std::sync::mpsc::Sender<(ContactId, String, i64)> {
    // Append `chunk` to the command's unsent-delta buffer (creating it on first output), bounding memory by trimming the FRONT with an explicit dropped-byte count. Wake only on the empty→occupied edge so a spewing build can't flood the event loop — the UI drain reads the buffer at its own pace.
    fn push_delta(
        partials: &std::sync::Mutex<std::collections::HashMap<ContactId, BridgeEmit>>,
        contact: ContactId,
        ts: i64,
        seq: u64,
        chunk: &str,
        fin: Option<i32>,
        host: &str,
        cwd: &str,
    ) -> bool {
        let mut m = partials.lock().unwrap();
        let e = m.entry(contact).or_insert_with(|| BridgeEmit { contact, target: ts, seq, body: String::new(), dropped: 0, fin: None, host: host.to_string(), cwd: cwd.to_string() });
        let fresh = e.body.is_empty() && e.fin.is_none();
        e.target = ts;
        e.seq = seq;
        e.body.push_str(chunk);
        e.fin = fin.or(e.fin);
        e.host = host.to_string();
        e.cwd = cwd.to_string();
        if e.body.len() > BRIDGE_BUF_MAX {
            // Char-boundary-safe front trim (the ⛅️✨🌎 lesson, 2026-08-28) — the cut is COUNTED, and the drain's elision marker names it.
            let mut cut = e.body.len() - BRIDGE_BUF_MAX;
            while cut < e.body.len() && !e.body.is_char_boundary(cut) {
                cut += 1;
            }
            e.body.drain(..cut);
            e.dropped += cut;
        }
        fresh
    }
    let (tx, rx) = std::sync::mpsc::channel::<(ContactId, String, i64)>();
    let spawned = std::thread::Builder::new()
        .name("bridge-shell".to_string())
        .spawn(move || {
            let mut shell: Option<BridgeShell> = None;
            let mut last_cwd = String::new();
            while let Ok((contact, cmd, ts)) = rx.recv() {
                if shell.is_none() {
                    match BridgeShell::spawn() {
                        Ok(s) => {
                            fg.lock().unwrap().insert(dev, s.child.id() as i32);
                            shell = Some(s);
                        }
                        Err(e) => {
                            push_delta(&partials, contact, ts, 1, &tr(Msg::BridgeShellStartFailed(&e.to_string())), Some(-1), "", "");
                            bridge_wake(&wake);
                            continue;
                        }
                    }
                }
                let sh = shell.as_mut().unwrap();
                let host = sh.host.clone();
                let cwd0 = last_cwd.clone();
                let mut seq: u64 = 0;
                let mut emitted_any = false;
                let res = sh.run_streaming(&cmd, |chunk| {
                    emitted_any = true;
                    seq += 1;
                    if push_delta(&partials, contact, ts, seq, chunk, None, &host, &cwd0) {
                        bridge_wake(&wake);
                    }
                });
                match res {
                    Ok((code, cwd, _)) => {
                        last_cwd = cwd.clone();
                        cwds.lock().unwrap().insert(dev, cwd.clone());
                        // "Finished" is a FIELD, not a message (Nick 2026-09-03): the exit code folds into whatever delta is still buffered and rides out on that frame. A command that never printed and failed still names itself; clean silent success stays an empty-bodied exit frame the client stamps without a bubble.
                        let text = if !emitted_any && code != 0 { tr(Msg::BridgeNoOutput(code)).into_owned() } else { String::new() };
                        push_delta(&partials, contact, ts, seq + 1, &text, Some(code), &host, &cwd);
                        bridge_wake(&wake);
                    }
                    Err(e) => {
                        // Registry absence = a deliberate Reset killed us — the client wiped its screen, so a death notice would land as a stray bubble in a fresh session. A REAL death (bash exited, crashed) reports once and the next command respawns.
                        let was_registered = fg.lock().unwrap().remove(&dev).is_some();
                        if was_registered {
                            push_delta(&partials, contact, ts, seq + 1, &tr(Msg::BridgeShellDied(&e)), Some(-1), &host, &cwd0);
                            bridge_wake(&wake);
                        }
                        return;
                    }
                }
            }
        });
    if let Err(e) = spawned {
        crate::logf!("BRIDGE: worker thread spawn failed: {}", e);
    }
    tx
}

/// One persistent shell for a sibling's bridge session (host side). A single long-lived `bash` process — spawned on the first command, reused for every command after — so working directory, exported vars, and shell state persist exactly like a real terminal, instead of a fresh `bash -lc` per command (Nick, 2026-08-22). stderr is merged into stdout in-shell (`exec 2>&1`); a per-session random sentinel marks each command's output boundary + exit code + cwd — shell-internal plumbing between this process and its own child, it NEVER touches the Photon wire (the wire carries typed VSF fields only). There is deliberately NO command timeout: busy is not wedged, liveness is the child process existing, and the operator holds the interrupt (the 30s reset both lied about killing the command AND orphaned a running deploy — field 2026-08-23). Shell DEATH is the one respawn edge.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
pub(super) struct BridgeShell {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    lines: std::sync::mpsc::Receiver<Option<String>>,
    sentinel: String,
    host: String,
}

#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
impl BridgeShell {
    const MAX_OUT: usize = 12 * 1024;

    fn spawn() -> std::io::Result<BridgeShell> {
        use std::io::{Read, Write};
        use std::process::{Command, Stdio};
        let mut child = Command::new("bash")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()) // merged into stdout below via `exec 2>&1`
            .spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        // Reader thread with the CR line discipline: `\n` commits a line; a bare `\r` (progress-bar redraw) RESETS the pending line so an animation collapses to its latest state instead of a thousand ghost frames; `\r\n` is a plain terminator. EOF (shell died) sends one `None` and ends.
        std::thread::spawn(move || {
            let mut stdout = stdout;
            let mut buf = [0u8; 4096];
            let mut line: Vec<u8> = Vec::new();
            let mut pending_cr = false;
            loop {
                let n = match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        if !line.is_empty() {
                            let _ = tx.send(Some(String::from_utf8_lossy(&line).into_owned()));
                        }
                        let _ = tx.send(None);
                        return;
                    }
                    Ok(n) => n,
                };
                for &b in &buf[..n] {
                    if pending_cr {
                        pending_cr = false;
                        if b == b'\n' {
                            if tx.send(Some(String::from_utf8_lossy(&line).into_owned())).is_err() {
                                return;
                            }
                            line.clear();
                            continue;
                        }
                        line.clear();
                    }
                    match b {
                        b'\n' => {
                            if tx.send(Some(String::from_utf8_lossy(&line).into_owned())).is_err() {
                                return;
                            }
                            line.clear();
                        }
                        b'\r' => pending_cr = true,
                        _ => line.push(b),
                    }
                }
            }
        });
        let sentinel = format!("__PHOTON_BRIDGE_{:016x}__", rand::random::<u64>());
        // Init: merge stderr→stdout, aliases best-effort (guarded .bashrc often skips non-interactive, so force expand_aliases + source explicitly), prompt silenced, then a bare `cd` — the spawned bash inherits PHOTON'S OWN cwd, which is whatever the launch path bequeathed (`/` from the Dock, the repo after a dev.sh reload, luck from autostart); every terminal starts at ~ and so does this one. `true` gives the priming run below a clean exit to read up to.
        // NON-INTERACTIVE HARDENING (field 2026-08-26, the git pull that "hung"): anything that opens an editor, a pager, or a credential prompt in a shell with no terminal waits forever with zero output. git merge → editor was the live case; pagers and apt prompts are the same class. Every such tool gets told the truth: no editor, no pager, no prompts.
        writeln!(
            stdin,
            "exec 2>&1; shopt -s expand_aliases 2>/dev/null; [ -f ~/.bashrc ] && source ~/.bashrc 2>/dev/null; [ -f ~/.bash_aliases ] && source ~/.bash_aliases 2>/dev/null; cd 2>/dev/null; PS1=''; PROMPT_COMMAND=''; export GIT_TERMINAL_PROMPT=0 GIT_EDITOR=true GIT_PAGER=cat PAGER=cat EDITOR=true VISUAL=true DEBIAN_FRONTEND=noninteractive; true"
        )?;
        let mut sh = BridgeShell {
            child,
            stdin,
            lines: rx,
            sentinel,
            host: String::new(),
        };
        // Drain init noise (bashrc chatter/errors) up to a priming sentinel so the FIRST real command's output starts clean, then capture the hostname once — bash defines $HOSTNAME even non-interactively, so no external binary runs.
        let _ = sh.run_streaming("true", |_| {});
        sh.host = sh
            .run_streaming("printf '%s\\n' \"$HOSTNAME\"", |_| {})
            .map(|(_, _, body)| body.trim().to_string())
            .unwrap_or_default();
        Ok(sh)
    }

    /// A `set -m` job-completion notice (`[1]+  Done   { cmd; }`) is shell chrome, not command output — screen it. Narrow shape: `[digits]` then an optional +/- then whitespace then a known verdict word.
    fn line_is_job_notice(line: &str) -> bool {
        let Some(rest) = line.strip_prefix('[') else {
            return false;
        };
        let Some(close) = rest.find(']') else {
            return false;
        };
        if close == 0 || !rest[..close].bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        let tail = rest[close + 1..].trim_start_matches(['+', '-']).trim_start();
        ["Done", "Terminated", "Interrupt", "Killed", "Stopped", "Exit", "Running"]
            .iter()
            .any(|v| tail.starts_with(v))
    }

    /// Feed one command as a FOREGROUND brace group — foreground because a backgrounded group is a SUBSHELL, and a subshell's `cd`/exports die with it (field 2026-08-23: every cd silently no-op'd and the operator found themselves in ~ believing they were in the repo — the persistent-shell property is the bridge's whole point). Stream every committed line to `emit` as the FULL accumulated snapshot; the closing sentinel carries exit code + cwd. Blocks until the command completes or the shell dies — no timeout by design; the interrupt path signals bash's child TREE from outside (bridge_interrupt_host), which needs no job announce at all. The tail is kept on overflow (a build's errors live at the END).
    fn run_streaming(
        &mut self,
        cmd: &str,
        mut emit: impl FnMut(&str),
    ) -> Result<(i32, String, String), String> {
        use std::io::Write;
        // The group brace closes on its OWN line so a trailing `#comment` in cmd can't swallow it; a multi-line paste rides inside the group unchanged. Foreground group = current shell = state persists.
        writeln!(self.stdin, "{{ {}", cmd).map_err(|e| e.to_string())?;
        writeln!(self.stdin, "}}").map_err(|e| e.to_string())?;
        writeln!(
            self.stdin,
            "printf '%s %d %s\\n' '{}' \"$?\" \"$PWD\"",
            self.sentinel
        )
        .map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        let mut body = String::new();
        let mut dropped = false;
        loop {
            let line = match self.lines.recv() {
                Ok(Some(l)) => l,
                Ok(None) | Err(_) => {
                    return Err(tr(Msg::ShellExited).into_owned());
                }
            };
            if let Some(rest) = line.trim_end().strip_prefix(&self.sentinel) {
                let rest = rest.trim_start();
                let (code_s, cwd) = rest.split_once(' ').unwrap_or((rest, ""));
                let final_body = if dropped {
                    tr(Msg::EarlierOutputDropped(&body)).into_owned()
                } else {
                    body
                };
                return Ok((code_s.parse().unwrap_or(-1), cwd.to_string(), final_body));
            }
            if Self::line_is_job_notice(&line) {
                continue;
            }
            // DELTA emit (Nick 2026-09-03): hand the caller exactly the newly committed line — the worker's buffer accumulates unsent output and the wire sends only what's missing, chain-ordered by hash links. The snapshot re-broadcast (whole body every window) is gone with its duplication.
            emit(&line);
            emit("\n");
            body.push_str(&line);
            body.push('\n');
            if body.len() > Self::MAX_OUT {
                let mut cut = body.len() - Self::MAX_OUT;
                // MAX_OUT is a byte budget but body is UTF-8 — a raw byte cut can land inside a multibyte char and panic String::drain on is_char_boundary (field 2026-08-28: wrangler's ⛅️✨🌎 overflowed a bridge deploy, the cut split an emoji, photon panicked mid-`git push` and took the bridge — and the deploy — down, then a hard-crash-mid-write left the vault degraded). Snap the cut UP to the next char boundary so the retained tail stays ≤ MAX_OUT.
                while cut < body.len() && !body.is_char_boundary(cut) {
                    cut += 1;
                }
                body.drain(..cut);
                dropped = true;
            }
        }
    }

    #[cfg(test)]
    pub(super) fn job_notice_screen_for_tests(line: &str) -> bool {
        Self::line_is_job_notice(line)
    }

    // (The old whole-session `kill` — SIGKILL the descendant tree then bash — is deleted: nothing called it since the Stop ladder took over per-command signalling (SIGINT→TERM→KILL against the job's own process group), which is strictly better — the session and its cwd survive a stopped command.)
}

#[cfg(all(test, unix, not(target_os = "android"), not(target_os = "redox")))]
mod tests {
    use super::*;

    /// The job-notice screen eats exactly bash's `set -m` chrome and nothing a command plausibly prints.
    #[test]
    fn job_notice_screen_is_narrow() {
        assert!(BridgeShell::job_notice_screen_for_tests("[1]+  Done                    { ls; }"));
        assert!(BridgeShell::job_notice_screen_for_tests("[2]-  Terminated              { sleep 99; }"));
        assert!(BridgeShell::job_notice_screen_for_tests("[12] Running { x; } &"));
        assert!(!BridgeShell::job_notice_screen_for_tests("[ok] Done in 3s"));
        assert!(!BridgeShell::job_notice_screen_for_tests("Done"));
        assert!(!BridgeShell::job_notice_screen_for_tests("[1] some array output"));
        assert!(!BridgeShell::job_notice_screen_for_tests("plain build line"));
    }
}

impl PhotonApp {
    /// Enter the add-device (pairing-words) flow. Was the interim Ready-orb action; now reached from the Fleet page's "Add device" pill. Spawns the bindreq watch so the candidate set is live before the first keystroke.
    /// Open a command conversation with a specific sibling DEVICE (the Bridge button). Siblings aren't listed as ordinary conversations, but they ARE contacts — find the one carrying this device pubkey and open it, so the chat-as-shell path (`$ cmd`) has a per-device surface. A seed hint message is inserted the first time so the screen isn't blank.
    pub(super) fn open_bridge_conversation(&mut self, device: [u8; 32]) {
        let idx = self
            .contacts
            .iter()
            .position(|c| c.is_sibling && c.device_key() == Some(device));
        let Some(ci) = idx else {
            self.ready_toast = Some(tr(Msg::DeviceNotSibling).into_owned());
            self.ready_toast_screen = None;
            crate::log("BRIDGE: no sibling contact for that device — cannot open");
            return;
        };
        // FRESH SESSION on open (Nick 2026-08-22): the terminal is ephemeral, so opening WIPES the on-screen rows, and any stale in-flight command frames are abandoned via LANE ROTATION — never a bare pending clear. Each frame links the previous frame's hash, so clearing pending mid-chain destroys the only copies of frames the peer still needs to link: the peer gap-buffers everything after the hole forever ('expected prev X — buffering (ahead of us)') and nothing ever ACKs again (field 2026-08-22, the no-ACK wedge THIS comment replaces). rotate_our_lane is the sanctioned abandon: retire the dead lane wholesale, mint a fresh one; the peer materializes it from the first frame's wire label and links from its ANCHOR — no hole possible. Safe for ephemeral rows because sibling frames are anchor-only (no strand ever references a wiped row). The host shell also resets (below) so cwd/env start clean.
        if let Some(conv) = self.conv_mut_of(ci) {
            conv.messages.clear();
        }
        if let Some(fid) = self.contacts.get(ci).and_then(|c| c.friendship_id) {
            if let Some((_, chains)) = self
                .friendship_chains
                .iter_mut()
                .find(|(id, _)| *id == fid)
            {
                // Rotation only when frames are actually in flight — a drained lane IS a fresh session at the chain level, and needless rotation grows the peer's lane-label set for nothing.
                if !chains.pending_messages.is_empty() {
                    if let Some((dead, fresh, retired)) = chains.rotate_our_lane() {
                        crate::logf!("BRIDGE: open with {} stale in-flight frame(s) — rotated lane {}... to {}... (fresh session discards them cleanly)", retired, hex::encode(&dead[..4]), hex::encode(&fresh[..4]));
                    }
                }
            }
        }
        // A fresh session starts unoriented: the locus strip fills from the first output's typed fields, and no Stop escalation carries over.
        self.bridge_locus = None;
        self.bridge_int = None;
        #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
        self.send_bridge_reset(ci);
        self.open_conversation_with(ci);
        self.state = AppState::Conversation;
        self.conv_topbar_off = 0.0;
                self.conv_blind_h = [0.0; 3];
                self.conv_blind_edge = f32::MAX;
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
    pub(super) fn run_bridge_command_chat(&mut self, ci: usize, cmd: &str, cmd_ts: i64) {
        let Some((contact, dev)) = self.contacts.get(ci).and_then(|c| Some((c.id, c.device_key()?))) else {
            return;
        };
        self.ensure_bridge_exec();
        if let Some(tx) = self.bridge_cmd_tx.as_ref() {
            let _ = tx.send(BridgeJob::Run(contact, dev, cmd.to_string(), cmd_ts));
        }
    }

    /// A BridgeCtl row arrived (the operator pressed Stop): signal the in-flight command's descendant tree — never bash, so the session and its cwd survive the interrupt. A late arrival after completion finds an empty tree and is a natural no-op.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn bridge_interrupt_host(&mut self, ci: usize, sig: u64, target: i64) {
        let Some(dev) = self.contacts.get(ci).and_then(|c| c.device_key()) else {
            return;
        };
        let pid = self
            .bridge_fg
            .as_ref()
            .map(|fg| fg.lock().unwrap().get(&dev).copied().unwrap_or(0))
            .unwrap_or(0);
        let tree = if pid > 0 { bridge_child_tree(pid) } else { Vec::new() };
        if tree.is_empty() {
            // ANSWER the no-op (field 2026-08-26, "not sure it stopped and no way to tell"): the operator pressed Stop and this host holds nothing to signal — commonest after a host restart orphaned the stream (a self-restarting deploy). A tiny final for the target says so and ends the client's in-flight state.
            crate::log("BRIDGE: interrupt arrived with no command in flight — answering with a no-op final");
            // seq None on purpose: the replace arm treats a seq-less final as fill-if-unfinished — it completes a row the client still shows as running, but never clobbers one already finished (e.g. by the client's own stream-loss stamp).
            // delta: true is load-bearing (field 2026-09-20): without it the client took the legacy whole-snapshot arm and REPLACED the command's entire transcript with this one-line verdict — "Stop clears the last message".
            let wire = crate::network::message_package::BridgeWire {
                exit: Some(-1),
                delta: true,
                ..Default::default()
            };
            self.send_chain_message(
                ci,
                &tr(Msg::StopReceivedIdle),
                false,
                Some((crate::types::RefKind::BridgeOut, target)),
                Some(wire),
            );
            return;
        }
        let sig = match sig {
            15 => libc::SIGTERM,
            9 => libc::SIGKILL,
            _ => libc::SIGINT,
        };
        crate::logf!(
            "BRIDGE: interrupt from operator — signal {} to {} process(es) under the shell",
            sig,
            tree.len()
        );
        for c in tree {
            unsafe {
                libc::kill(c, sig);
            }
        }
    }

    /// The Stop button (client side, ANY platform — a phone must be able to stop a runaway build too): find the in-flight command, escalate SIGINT → SIGTERM → SIGKILL per press, and send the typed BridgeCtl row targeting it. Hidden control row; the host signals the job's own process group, so the session and its cwd survive.
    pub(super) fn bridge_send_interrupt(&mut self, ci: usize) {
        let Some(t) = self.bridge_inflight_target(ci) else {
            return;
        };
        let level = match self.bridge_int {
            Some((t0, l)) if t0 == t => (l + 1).min(2),
            _ => 0,
        };
        self.bridge_int = Some((t, level));
        let sig = [2u64, 15, 9][level as usize];
        let wire = crate::network::message_package::BridgeWire {
            sig: Some(sig),
            ..Default::default()
        };
        crate::logf!(
            "BRIDGE: Stop pressed — sending signal {} for the in-flight command",
            sig
        );
        self.send_chain_message(
            ci,
            " ",
            true,
            Some((crate::types::RefKind::BridgeCtl, t)),
            Some(wire),
        );
        // THE OPERATOR'S ESCAPE (field 2026-08-26): the KILL press releases the prompt LOCALLY, depending on nobody. The offline watcher misses a FAST-restarting host (a deploy relaunches photon before the 3-timeout offline verdict), the restarted host knows nothing of the old stream, and an old-build host answers Stop with a log line the client never sees — three presses must always get the terminal back. Edge-triggered (the press), no timer; bridge_seq goes terminal so any straggler final is swallowed, and the notice says the command may well still be running.
        if level == 2 {
            let Some(conv) = self.conv_mut_of(ci) else {
                return;
            };
            let notice = tr(Msg::NoResponseToStop);
            if let Some(row) = conv.messages.iter_mut().find(|m| {
                !m.is_outgoing && m.reference == Some((crate::types::RefKind::BridgeOut, t))
            }) {
                row.content.push_str(&notice);
                row.bridge_exit = Some(-1);
                row.bridge_seq = u64::MAX;
            } else {
                let mut msg = crate::types::ChatMessage::new_with_timestamp(
                    notice.trim_start().to_string(),
                    false,
                    vsf::eagle_time_oscillations(),
                );
                msg.reference = Some((crate::types::RefKind::BridgeOut, t));
                msg.bridge_exit = Some(-1);
                msg.bridge_seq = u64::MAX;
                conv.insert_message_sorted(msg);
            }
            self.bridge_int = None;
            self.scene_dirty = true;
            crate::log("BRIDGE: third Stop press — prompt released locally");
        }
    }

    /// CLIENT side, any platform: a command is in flight but its HOST has gone dark — stamp the streamed row closed with a loud notice, once. The deploy case (field 2026-08-26): a self-restarting command kills the host's photon, the bridge worker and its stream die with it (the command itself survives detached), and no final can ever arrive — the client sat frozen on the first seconds of output with a Stop button that no-ops. The offline verdict is the edge; stamping `bridge_exit` ends the in-flight state idempotently (the stamp itself makes the next pass a no-op).
    pub(super) fn bridge_watch_stream_loss(&mut self) {
        let lost: Vec<(usize, i64)> = self
            .contacts
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_sibling && !c.is_online && c.presence_probed)
            .filter_map(|(ci, _)| self.bridge_inflight_target(ci).map(|t| (ci, t)))
            .collect();
        for (ci, t) in lost {
            let Some(conv) = self.conv_mut_of(ci) else {
                continue;
            };
            let notice = tr(Msg::StreamLost);
            if let Some(row) = conv.messages.iter_mut().find(|m| {
                !m.is_outgoing && m.reference == Some((crate::types::RefKind::BridgeOut, t))
            }) {
                row.content.push_str(&notice);
                row.bridge_exit = Some(-1);
            } else {
                // No output ever arrived — materialize the verdict row so the operator isn't staring at a silent faint command forever.
                let mut msg = crate::types::ChatMessage::new_with_timestamp(
                    notice.trim_start().to_string(),
                    false,
                    vsf::eagle_time_oscillations(),
                );
                msg.reference = Some((crate::types::RefKind::BridgeOut, t));
                msg.bridge_exit = Some(-1);
                conv.insert_message_sorted(msg);
            }
            if self.bridge_int.map_or(false, |(t0, _)| t0 == t) {
                self.bridge_int = None;
            }
            self.scene_dirty = true;
            crate::logf!("BRIDGE: host went offline with a command in flight (target {}) — stream marked lost", t);
        }
    }

    /// The in-flight command, if any: the newest outgoing BridgeCmd row that is DELIVERED (the ACK proves the host holds it — Nick 2026-08-27: the row's own testimony, not a guess) but has no FINAL yet. An undelivered command isn't running anywhere — it's a queued send, the give-up verdict owns its fate, and the lane's hash-chain executes commands in order regardless — so it must never hold the prompt (the zombie-gate class). Drives the Stop button's visibility.
    pub(super) fn bridge_inflight_target(&self, ci: usize) -> Option<i64> {
        let conv = self.conv_of(ci)?;
        let cmd = conv.messages.iter().rev().find(|m| {
            m.is_outgoing
                && m.delivered
                && matches!(m.reference, Some((crate::types::RefKind::BridgeCmd, _)))
        })?;
        let done = conv.messages.iter().any(|m| {
            m.reference == Some((crate::types::RefKind::BridgeOut, cmd.timestamp))
                && m.bridge_exit.is_some()
        });
        if done {
            None
        } else {
            Some(cmd.timestamp)
        }
    }

    /// Client OPENED the bridge → tell the host to drop its shell for us so the next command starts a FRESH session (Nick 2026-08-22). Fire-and-forget over the chain as a hidden BridgeReset control row.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn send_bridge_reset(&mut self, ci: usize) {
        // A minimal payload; the TYPED reference carries the meaning and the row is hidden either way.
        self.send_chain_message(ci, " ", false, Some((crate::types::RefKind::BridgeReset, 0)), None);
    }

    /// Host received a BridgeReset (the peer opened the bridge) → drop that sibling's shell so the next command respawns fresh.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn reset_bridge_shell(&mut self, ci: usize) {
        let Some(dev) = self.contacts.get(ci).and_then(|c| c.device_key()) else {
            return;
        };
        // Symmetric with the client's open: abandon any stale in-flight OUTPUT frames via lane rotation (NEVER a bare pending clear — that leaves a mid-chain hash hole the peer buffers behind forever; see open_bridge_conversation). Replies from the prior session stop retransmitting into the freshly-wiped screen.
        if let Some(fid) = self.contacts.get(ci).and_then(|c| c.friendship_id) {
            if let Some((_, chains)) = self
                .friendship_chains
                .iter_mut()
                .find(|(id, _)| *id == fid)
            {
                if !chains.pending_messages.is_empty() {
                    if let Some((dead, fresh, retired)) = chains.rotate_our_lane() {
                        crate::logf!("BRIDGE: reset with {} stale in-flight output frame(s) — rotated lane {}... to {}...", retired, hex::encode(&dead[..4]), hex::encode(&fresh[..4]));
                    }
                }
            }
        }
        self.ensure_bridge_exec();
        if let Some(tx) = self.bridge_cmd_tx.as_ref() {
            let _ = tx.send(BridgeJob::Reset(dev));
        }
    }

    /// Lazily spawn the off-thread bridge DISPATCHER (routes jobs to one worker thread per sibling device — see spawn_bridge_worker). No-op once running.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    fn ensure_bridge_exec(&mut self) {
        if self.bridge_cmd_tx.is_some() {
            return;
        }
        let wake = self.event_proxy.clone();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<BridgeJob>();
        // ONE shared per-command delta buffer carries everything — output AND the exit that folds into the last frame (no separate final channel; "finished" is a field, not a message).
        let partials: std::sync::Arc<
            std::sync::Mutex<std::collections::HashMap<ContactId, BridgeEmit>>,
        > = Default::default();
        let fg: BridgeFgMap = Default::default();
        let cwds: BridgeCwdMap = self.bridge_cwds.get_or_insert_with(Default::default).clone();
        self.bridge_partials = Some(partials.clone());
        self.bridge_fg = Some(fg.clone());
        std::thread::Builder::new()
            .name("bridge-exec".to_string())
            .spawn(move || {
                let mut workers: crate::linear_map::LinearMap<
                    [u8; 32],
                    std::sync::mpsc::Sender<(ContactId, String, i64)>,
                > = crate::linear_map::LinearMap::new();
                while let Ok(job) = cmd_rx.recv() {
                    match job {
                        BridgeJob::Reset(dev) => {
                            // Deregister FIRST (the worker reads absence as "deliberate reset — hush"), then kill the command's descendant tree AND bash: bash's death alone orphans a running command invisibly, the exact lie the old timeout told (field 2026-08-23).
                            workers.remove(&dev);
                            if let Some(pid) = fg.lock().unwrap().remove(&dev) {
                                for c in bridge_child_tree(pid) {
                                    unsafe {
                                        libc::kill(c, libc::SIGKILL);
                                    }
                                }
                                if pid > 0 {
                                    unsafe {
                                        libc::kill(pid, libc::SIGKILL);
                                    }
                                }
                            }
                        }
                        BridgeJob::Run(contact, dev, cmd, ts) => {
                            crate::logf!("BRIDGE: running command from sibling: {}", cmd);
                            let alive = workers
                                .get(&dev)
                                .map(|tx| tx.send((contact, cmd.clone(), ts)).is_ok())
                                .unwrap_or(false);
                            if !alive {
                                let tx = spawn_bridge_worker(
                                    dev,
                                    partials.clone(),
                                    fg.clone(),
                                    cwds.clone(),
                                    wake.clone(),
                                );
                                let _ = tx.send((contact, cmd, ts));
                                workers.insert(dev, tx);
                            }
                        }
                    }
                }
            })
            .expect("spawn bridge-exec");
        self.bridge_cmd_tx = Some(cmd_tx);
    }

    /// Tick drain: ship each command's accumulated DELTA over the durable chain — spool up to the 1s window, broadcast what's missing, and if nothing spooled, send nothing (ten silent minutes = zero frames). Every frame carries the typed locus + seq; the frame whose `exit` field is present IS the finish signal (no special end message — Nick 2026-09-03). UI thread, zero shell work — just the sends.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn drain_bridge_output(&mut self) {
        let Some(slots) = self.bridge_partials.clone() else {
            return;
        };
        // Put an unsent delta BACK, prepending it to whatever the worker spooled meanwhile — content is never dropped by a parked or failed send, order is preserved, and the exit survives the merge.
        let put_back = |slots: &std::sync::Mutex<std::collections::HashMap<ContactId, BridgeEmit>>, e: BridgeEmit| {
            let mut m = slots.lock().unwrap();
            match m.entry(e.contact) {
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    let cur = o.get_mut();
                    let mut body = e.body;
                    body.push_str(&cur.body);
                    cur.body = body;
                    cur.dropped += e.dropped;
                    cur.fin = cur.fin.or(e.fin);
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(e);
                }
            }
        };
        let taken: Vec<BridgeEmit> = { slots.lock().unwrap().drain().map(|(_, e)| e).collect() };
        for e in taken {
            if e.body.is_empty() && e.fin.is_none() {
                continue;
            }
            // The sibling may have been removed while its command ran — its output has no conversation to land in.
            let Some(ci) = self.ci_of(&e.contact) else {
                crate::log("BRIDGE: output dropped — its sibling was removed while the command ran");
                self.bridge_partial_inflight.remove(&e.contact);
                self.bridge_partial_sent.remove(&e.contact);
                continue;
            };
            let is_final = e.fin.is_some();
            if !is_final {
                // THE ONE TIMER (Nick's grant, 2026-08-31): deltas reach the wire at most once per second per conversation — the spool collapses bursts, this paces the broadcast. The exit-carrying frame is never paced.
                let recently = self
                    .bridge_partial_sent
                    .get(&e.contact)
                    .map_or(false, |t| t.elapsed() < std::time::Duration::from_secs(1));
                // ONE delta in flight per feed, gated on ITS OWN ACK edge — the whole-lane pending count starved the feed behind fleet-sync chatter (the silent v82 deploy, 2026-09-03). The ACK arriving is the wake edge that ships the next spool; a parked spool keeps accumulating, nothing is lost.
                let prev_unacked = self.bridge_partial_inflight.get(&e.contact).map_or(false, |&et| {
                    self.contacts
                        .get(ci)
                        .and_then(|c| c.friendship_id)
                        .and_then(|fid| self.friendship_chains.iter().find(|(id, _)| *id == fid))
                        .map_or(false, |(_, ch)| ch.pending_messages.iter().any(|m| m.eagle_time == et))
                });
                if recently || prev_unacked {
                    put_back(&slots, e);
                    continue;
                }
            }
            // A buffer-bound trim is named, never silent: the elision marker carries the exact byte count that fell off the front.
            let body = if e.dropped > 0 {
                tr(Msg::BridgeElided { bytes: e.dropped, output: &e.body }).into_owned()
            } else {
                e.body.clone()
            };
            let wire = crate::network::message_package::BridgeWire {
                host: (!e.host.is_empty()).then(|| e.host.clone()),
                cwd: (!e.cwd.is_empty()).then(|| e.cwd.clone()),
                seq: Some(e.seq),
                exit: e.fin.map(|c| c as i64),
                sig: None,
                delta: true,
                    pigeon: None,
            };
            if is_final {
                // The exit-carrying delta rides the full durable path (host row + retransmit + held-row re-serve) — it is the one frame that must survive.
                self.bridge_partial_inflight.remove(&e.contact);
                let sent = self.send_chain_message(
                    ci,
                    &body,
                    false,
                    Some((crate::types::RefKind::BridgeOut, e.target)),
                    Some(wire),
                );
                let exit_code = e.fin.unwrap_or(i32::MIN);
                let verdict = if sent { "sent" } else { "HELD — send_chain_message refused it; it re-serves with the held rows" };
                crate::logf!("BRIDGE: final for target {} (seq {}, exit {}, {} byte(s)) {}", e.target, e.seq, exit_code, e.body.len(), verdict);
            } else {
                // Mid-command deltas ride chain_transmit directly with an eagle_time minted HERE so the own-ACK gate can watch this exact frame leave pending. A refused send (window full, no address yet) puts the spool back intact — the transcript never loses a byte to flow control.
                let et = vsf::eagle_time_oscillations();
                if self.chain_transmit(ci, &body, et, Some((crate::types::RefKind::BridgeOut, e.target)), Some(&wire)) {
                    self.bridge_partial_inflight.insert(e.contact, et);
                    self.bridge_partial_sent.insert(e.contact, std::time::Instant::now());
                } else {
                    put_back(&slots, e);
                }
            }
        }
    }

    /// Turn unattended mode on/off. ON writes the marker AND (also requires background/autostart so a reboot actually relaunches photon) refreshes the capsule from the live session. OFF removes the marker and shreds the capsule.
    pub(super) fn set_unattended(&mut self, on: bool) {
        self.unattended_on = on;
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

// ── Bridge PIGEONS, v2 (docs/PT.md "Pigeons, v2"; Nick 2026-10-07: "what's the proper way to send pigeons… Photon Transport, with the lettering rotation and all. Fix it!"): a file dropped on the bridge lands in the host shell's cwd. PT moves each chunk as its own lettered stream; every chunk SELF-DESCRIBES so the first to arrive opens the host's zero-bitmap spool; the host REPAIRS by asking for what its spool lacks (`pigeon_want`, read off the zero-scan, never stored); the client keeps its copy until the host reports every slot held.

/// The spool directory on the host.
fn pigeon_spool_dir() -> std::path::PathBuf {
    crate::storage::runtime_dir().join("pigeons")
}

/// BLAKE3 of a file, streamed (never the whole file in RAM).
fn hash_file(path: &std::path::Path) -> std::io::Result<[u8; 32]> {
    let mut h = blake3::Hasher::new();
    std::io::copy(&mut std::fs::File::open(path)?, &mut h)?;
    Ok(*h.finalize().as_bytes())
}

impl PhotonApp {
    /// CLIENT: drop → pigeon. The read, the hash and the vault store run on the seal worker (a big file would stall the window); `drain_pigeon_prepared` announces it and sends the chunks.
    pub(super) fn send_bridge_pigeon(&mut self, ci: usize, path: &std::path::Path) {
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(contact) = self.contacts.get(ci).map(|c| c.id) else {
            return;
        };
        if name.is_empty() {
            return;
        }
        let (tx, wake, path) = (self.pigeon_prepared_tx.clone(), self.event_proxy.clone(), path.to_path_buf());
        queue_job(&self.seal_job_tx, move || {
            // Hash FIRST: whether the vault already held these bytes decides whether the copy may be shed once the host has it (a file that was also an attachment must survive its pigeon).
            let hash = match hash_file(&path) {
                Ok(h) => h,
                Err(e) => {
                    crate::logf!("PIGEON: could not read the dropped file: {}", e);
                    return;
                }
            };
            let was_held = crate::storage::blob_present_probe_now(&hash);
            match crate::storage::blob_store_file(&seed, &path) {
                Ok((stored, manifest)) if stored == hash => {
                    let size = manifest.as_ref().map(|m| m.size).unwrap_or_else(|| std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0));
                    let (chunks, single) = match manifest {
                        Some(m) => (m.chunks, false),
                        None => (vec![hash], true),
                    };
                    let _ = tx.send(super::PigeonPrepared { contact, name, hash, size, chunks, single, was_held });
                    bridge_wake(&wake);
                }
                Ok(_) => crate::log("PIGEON: the dropped file changed while it was read — drop it again"),
                Err(e) => crate::logf!("PIGEON: could not store the dropped file: {}", e),
            }
        });
    }

    /// CLIENT tick drain: a stored drop → its announcement row, its bar, its outbound record, and the first send of every chunk.
    pub(super) fn drain_pigeon_prepared(&mut self) {
        while let Ok(p) = self.pigeon_prepared_rx.try_recv() {
            let Some(ci) = self.ci_of(&p.contact) else {
                crate::log("PIGEON: the sibling was removed before its drop was ready — not sent");
                continue;
            };
            let Some(device) = self.contacts[ci].device_key() else {
                continue;
            };
            let wire = crate::network::message_package::BridgeWire {
                pigeon: Some(crate::network::message_package::BridgePigeon { name: p.name.clone(), hash: p.hash, size: p.size }),
                ..Default::default()
            };
            self.send_chain_message(ci, &p.name, false, Some((crate::types::RefKind::BridgePigeon, 0)), Some(wire));
            // The row's bar starts empty here and fills from the host's pigeon_ack frames — the host's spool is the only truth about what has landed.
            self.pigeon_progress.insert(p.hash, super::PigeonProgress { device, name: p.name.clone(), got: 0, of: p.chunks.len() as u32, at: std::time::Instant::now() });
            crate::logf!("PIGEON: {} ({} bytes, {} chunk(s)) announced — the copy stays here until the host holds every chunk", p.name, p.size, p.chunks.len());
            self.pigeon_outbound.insert(p.hash, super::PigeonOutbound { device, name: p.name, size: p.size, chunks: p.chunks, single: p.single, was_held: p.was_held });
            self.dispatch_pigeon_chunks(ci, p.hash, None);
        }
    }

    /// CLIENT: seal and send chunks of a pigeon — all of them (`which` = None, the first send) or exactly the slots a repair ask named. Every frame carries the size and the name sealed under the fleet key, so whichever lands first opens the host's spool. After a restart (no outbound record) the vault's own manifest still serves a repair; the name is then omitted (the host already has its spool).
    pub(super) fn dispatch_pigeon_chunks(&mut self, ci: usize, hash: [u8; 32], which: Option<Vec<u32>>) {
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let (device, addr_pair, relay_to, token) = {
            let Some(c) = self.contacts.get(ci) else {
                return;
            };
            let Some(device) = c.device_key() else {
                return;
            };
            // Sibling exchanges seal under the fleet key; the token is a plain discriminator, so the handle hash serves (as attach_fetch does).
            (device, c.race_addrs(), relay_unless_direct_trusted(c, crate::network::udp::get_local_ip()), c.handle_hash)
        };
        let (peer_addr, alt_addr) = addr_pair.unwrap_or((crate::network::status::RELAY_ADDR, None));
        let Some(wire_key) = self.fleet_key_cached() else {
            crate::log("PIGEON: no fleet key — cannot seal");
            return;
        };
        let (Some(kp), Some(checker)) = (self.device_keypair.as_ref(), self.status_checker.as_ref()) else {
            return;
        };
        let (kp_pub, kp_sec) = (*kp.public.as_bytes(), *kp.secret.as_bytes());
        let dispatch = checker.history_dispatch();
        let outbound = self.pigeon_outbound.get(&hash).map(|o| (o.name.clone(), o.size, o.chunks.clone(), o.single));
        queue_job(&self.seal_job_tx, move || {
            let (name, mut size, chunks, single) = match outbound {
                Some((n, s, c, one)) => (Some(n), s, c, one),
                None => match crate::storage::blob_manifest(&hash) {
                    Some(m) => (None, m.size, m.chunks, false),
                    None => (None, 0, vec![hash], true),
                },
            };
            let sealed_name = name.as_ref().and_then(|n| kete::encrypt_bytes(n.as_bytes(), &wire_key).ok());
            let slots: Vec<usize> = match which.as_ref() {
                Some(w) => w.iter().map(|i| *i as usize).filter(|i| *i < chunks.len()).collect(),
                None => (0..chunks.len()).collect(),
            };
            let mut sent = 0usize;
            for i in slots.iter().copied() {
                let plain = if single { crate::storage::blob_load(&seed, &chunks[i]) } else { crate::storage::blob_chunk_load(&chunks[i]) };
                let Some(plain) = plain else {
                    crate::logf!("PIGEON: chunk {} of {}… is not on this device — cannot serve it", i, hex::encode(&hash[..4]));
                    continue;
                };
                if single && size == 0 {
                    size = plain.len() as u64;
                }
                match kete::encrypt_bytes(&plain, &wire_key).and_then(|sealed| crate::network::fgtw::protocol::build_pigeon_chunk_vsf(&token, &hash, i as u32, sealed, size, sealed_name.as_deref(), &kp_pub, &kp_sec)) {
                    Ok(v) => {
                        let _ = dispatch.send(crate::network::status::HistorySendRequest { peer_addr, alt_addr, recipient_pubkey: device, vsf_bytes: v, relay_to: relay_to.clone(), tag: None });
                        sent += 1;
                    }
                    Err(e) => crate::logf!("PIGEON: chunk {} frame build failed: {}", i, e),
                }
            }
            crate::logf!("PIGEON: {}… — {} of {} chunk(s) sent{}", hex::encode(&hash[..4]), sent, slots.len(), if which.is_some() { " (repair)" } else { "" });
        });
    }

    /// CLIENT: the host's repair ask — serve exactly the slots it named. Only a device of this fleet may ask.
    pub(super) fn on_pigeon_want(&mut self, hash: [u8; 32], want: Vec<u32>, from: [u8; 32]) {
        let Some(ci) = self.contacts.iter().position(|c| c.is_sibling && c.device_key() == Some(from)) else {
            return;
        };
        crate::logf!("PIGEON: host {} asks for {} slot(s) of {}…", crate::fp(&from), want.len(), hex::encode(&hash[..4]));
        self.dispatch_pigeon_chunks(ci, hash, Some(want));
    }

    /// HOST: the announcement row landed — open (or resume) the spool under the fleet key, keyed to the announcing device. The spool may already be open (its first chunk beat the row): then the row only supplies the name, and a pigeon that was whole and nameless lands now.
    pub(super) fn on_pigeon_announced(&mut self, ci: usize, p: crate::network::message_package::BridgePigeon) {
        let Some(dev) = self.contacts.get(ci).and_then(|c| c.device_key()) else {
            return;
        };
        let Some(wire_key) = self.fleet_key_cached() else {
            crate::log("PIGEON: announced but no fleet key — cannot open a spool");
            return;
        };
        match self.pigeon_rx.open(&pigeon_spool_dir(), p.hash, p.name.clone(), p.size, crate::storage::BLOB_CHUNK_SIZE as u32, wire_key, dev) {
            Ok((got, of, resumed)) => {
                crate::logf!("PIGEON: spool open for {} ({} bytes) from {} — {} of {} chunk(s) held", p.name, p.size, crate::fp(&dev), got, of);
                self.pigeon_progress.insert(p.hash, super::PigeonProgress { device: dev, name: p.name.clone(), got, of, at: std::time::Instant::now() });
                if resumed && got < of {
                    self.send_pigeon_want(&p.hash);
                }
                self.try_land_pigeon(&p.hash);
            }
            Err(e) => crate::logf!("PIGEON: spool open failed for {}: {}", p.name, e),
        }
    }

    /// HOST: one sealed chunk off the wire. The FIRST chunk of an unknown pigeon opens its spool from the frame's own size and sealed name (any arrival order now lands); each accepted chunk advances the bar, answers the sender with a thinned pigeon_ack, and the last one lands the file.
    pub(super) fn on_pigeon_chunk(&mut self, hash: [u8; 32], index: u32, sealed: Vec<u8>, size: Option<u64>, sealed_name: Option<Vec<u8>>, signer: [u8; 32]) {
        if !self.pigeon_rx.holds(&hash) {
            if !self.contacts.iter().any(|c| c.is_sibling && c.device_key() == Some(signer)) {
                return;
            }
            let Some(size) = size else {
                crate::logf!("PIGEON: chunk {} of an unannounced pigeon from an older build — dropped (its announcement opens the spool)", index);
                return;
            };
            let Some(wire_key) = self.fleet_key_cached() else {
                return;
            };
            let name = sealed_name.and_then(|n| kete::decrypt_bytes(&n, &wire_key).ok()).and_then(|b| String::from_utf8(b).ok()).unwrap_or_default();
            match self.pigeon_rx.open(&pigeon_spool_dir(), hash, name.clone(), size, crate::storage::BLOB_CHUNK_SIZE as u32, wire_key, signer) {
                Ok((got, of, _)) => {
                    crate::logf!("PIGEON: spool opened by its first chunk for {} ({} bytes) from {} — {} of {} held", if name.is_empty() { "(name to follow)" } else { name.as_str() }, size, crate::fp(&signer), got, of);
                    self.pigeon_progress.insert(hash, super::PigeonProgress { device: signer, name, got, of, at: std::time::Instant::now() });
                }
                Err(e) => {
                    crate::logf!("PIGEON: spool open failed: {}", e);
                    return;
                }
            }
        }
        let landed = match self.pigeon_rx.chunk(&hash, index, &sealed, &signer) {
            Err(e) => {
                crate::logf!("PIGEON: chunk {} write failed: {}", index, e);
                None
            }
            Ok(v) => v,
        };
        // The host's running word back to the sender (pigeon_ack): on the landing EDGE, thinned by pigeon_ack_due so a 545-chunk pigeon costs ~65 tiny frames on the small-packet lane.
        if let Some((got, of)) = landed {
            let prev = self.pigeon_progress.get(&hash).map_or(0, |pp| pp.got);
            if let Some(pp) = self.pigeon_progress.get_mut(&hash) {
                pp.got = got;
                pp.of = of;
                pp.at = std::time::Instant::now();
                if pp.name.is_empty() {
                    pp.name = self.pigeon_rx.name(&hash).unwrap_or_default().to_string();
                }
                self.scene_dirty = true;
            }
            if pigeon_ack_due(prev, got, of) {
                self.send_pigeon_ack(&hash, got, of, signer);
            }
            if got >= of {
                self.try_land_pigeon(&hash);
            }
        }
    }

    fn send_pigeon_ack(&mut self, hash: &[u8; 32], got: u32, of: u32, to: [u8; 32]) {
        let Some(ci) = self.contacts.iter().position(|c| c.is_sibling && c.device_key() == Some(to)) else {
            return;
        };
        let (Some(kp), Some(checker)) = (self.device_keypair.as_ref(), self.status_checker.as_ref()) else {
            return;
        };
        let c = &self.contacts[ci];
        let relay_to = relay_unless_direct_trusted(c, crate::network::udp::get_local_ip());
        let (peer_addr, alt_addr) = c.race_addrs().unwrap_or((crate::network::status::RELAY_ADDR, None));
        match crate::network::fgtw::protocol::build_pigeon_ack_vsf(&c.handle_hash, hash, got, of, kp.public.as_bytes(), kp.secret.as_bytes()) {
            Ok(vsf_bytes) => checker.send_history(crate::network::status::HistorySendRequest { peer_addr, alt_addr, recipient_pubkey: to, vsf_bytes, relay_to, tag: None }),
            Err(e) => crate::logf!("PIGEON: ack frame build failed: {}", e),
        }
    }

    /// HOST: ask the sending device for the slots this pigeon's spool still lacks — read off the zero-scan now, never stored.
    pub(super) fn send_pigeon_want(&mut self, hash: &[u8; 32]) {
        let Some((want, dev)) = self.pigeon_rx.want(hash) else {
            return;
        };
        let Some(ci) = self.contacts.iter().position(|c| c.is_sibling && c.device_key() == Some(dev)) else {
            return;
        };
        let (Some(kp), Some(checker)) = (self.device_keypair.as_ref(), self.status_checker.as_ref()) else {
            return;
        };
        let c = &self.contacts[ci];
        let relay_to = relay_unless_direct_trusted(c, crate::network::udp::get_local_ip());
        let (peer_addr, alt_addr) = c.race_addrs().unwrap_or((crate::network::status::RELAY_ADDR, None));
        match crate::network::fgtw::protocol::build_pigeon_want_vsf(&c.handle_hash, hash, &want, kp.public.as_bytes(), kp.secret.as_bytes()) {
            Ok(vsf_bytes) => {
                checker.send_history(crate::network::status::HistorySendRequest { peer_addr, alt_addr, recipient_pubkey: dev, vsf_bytes, relay_to, tag: None });
                crate::logf!("PIGEON: asked {} for {} missing slot(s) of {}…", crate::fp(&dev), want.len(), hex::encode(&hash[..4]));
            }
            Err(e) => crate::logf!("PIGEON: want frame build failed: {}", e),
        }
    }

    /// HOST: a whole, named spool lands on the seal worker in the sibling's shell cwd; `drain_pigeon_landed` answers in the transcript.
    fn try_land_pigeon(&mut self, hash: &[u8; 32]) {
        let Some(inf) = self.pigeon_rx.take_complete(hash) else {
            return;
        };
        let from = inf.from_device();
        let seed = self.session.as_ref().map(|s| s.identity_seed);
        let contact = self.contacts.iter().find(|c| c.device_key() == Some(from)).map(|c| c.id);
        let (Some(seed), Some(contact)) = (seed, contact) else {
            crate::log("PIGEON: whole, but its sibling or our session is gone — not landed (the spool stays for the next session)");
            return;
        };
        // The landing directory: the sibling's shell cwd as of its last command, else the shell's starting directory (home).
        let cwd = self.bridge_cwds.as_ref().and_then(|m| m.lock().ok()).and_then(|m| m.get(&from).cloned()).filter(|c| !c.is_empty());
        let dir = cwd.map(std::path::PathBuf::from).or_else(dirs::home_dir).unwrap_or_else(|| std::path::PathBuf::from("."));
        let (tx, wake) = (self.pigeon_landed_tx.clone(), self.event_proxy.clone());
        queue_job(&self.seal_job_tx, move || {
            let dir_s = dir.to_string_lossy().into_owned();
            let landed = crate::network::pigeon::finalize_inflight(inf, &seed, &dir).unwrap_or(None);
            let _ = tx.send((contact, landed, dir_s));
            bridge_wake(&wake);
        });
    }

    /// HOST, on the drought cadence: once per session resume every spool a previous run left (and ask for the rest); then ask again for any pigeon that stopped moving, and let go of one that never answered — saying so in its transcript.
    pub(super) fn pigeon_repair_tick(&mut self) {
        if !self.pigeon_rediscovered {
            if let Some(k) = self.fleet_key_cached() {
                self.pigeon_rediscovered = true;
                for (hash, dev) in self.pigeon_rx.rediscover(&pigeon_spool_dir(), k) {
                    let (got, of) = self.pigeon_rx.progress(&hash).unwrap_or((0, 0));
                    let name = self.pigeon_rx.name(&hash).unwrap_or_default().to_string();
                    crate::logf!("PIGEON: resumed {} from a previous session — {} of {} held, asking {} for the rest", if name.is_empty() { "a nameless spool" } else { name.as_str() }, got, of, crate::fp(&dev));
                    self.pigeon_progress.insert(hash, super::PigeonProgress { device: dev, name, got, of, at: std::time::Instant::now() });
                    self.send_pigeon_want(&hash);
                }
            }
        }
        let (ask, gone) = self.pigeon_rx.stalled(std::time::Instant::now());
        for h in ask {
            crate::logf!("PIGEON: {}… stopped moving — asking for what is missing", hex::encode(&h[..4]));
            self.send_pigeon_want(&h);
        }
        for (h, name, from) in gone {
            crate::logf!("PIGEON: {} — no answer after {} asks; the spool stays on disk for the next session", name, crate::network::pigeon::MAX_REPAIR_ASKS);
            self.pigeon_progress.remove(&h);
            if let Some(ci) = self.contacts.iter().position(|c| c.is_sibling && c.device_key() == Some(from)) {
                let body = tr(Msg::PigeonStalled(&name)).into_owned();
                self.send_chain_message(ci, &body, false, Some((crate::types::RefKind::BridgeOut, 0)), None);
            }
        }
    }

    /// CLIENT: the host's word on a pigeon we dropped. Monotonic — a late or reordered ack never walks the bar back. Only a device this fleet knows may speak. WHOLE = the host holds every slot: the copy kept for repairs is shed (unless the vault held these bytes before the drop).
    pub(super) fn on_pigeon_ack(&mut self, content_hash: [u8; 32], got: u32, of: u32, from: [u8; 32]) {
        if !self.contacts.iter().any(|c| c.is_sibling && c.device_key() == Some(from)) {
            return;
        }
        let whole = of > 0 && got >= of;
        if let Some(pp) = self.pigeon_progress.get_mut(&content_hash) {
            if got > pp.got || of != pp.of {
                pp.got = got.max(pp.got);
                pp.of = of;
                pp.at = std::time::Instant::now();
                if whole {
                    crate::logf!("PIGEON: {} — host {} holds every one of {} chunk(s); landing next", pp.name, crate::fp(&from), pp.of);
                }
                super::bridge::bridge_wake(&self.event_proxy);
                self.scene_dirty = true;
                if whole {
                    // The bar is full; the host's "landed at …" row is the next thing the operator sees.
                    self.pigeon_progress.remove(&content_hash);
                }
            }
        }
        if whole {
            if let Some(o) = self.pigeon_outbound.remove(&content_hash) {
                if o.was_held {
                    crate::logf!("PIGEON: {} delivered — the vault held these bytes before the drop, so they stay", o.name);
                } else {
                    crate::logf!("PIGEON: {} delivered — the local copy is shed", o.name);
                    queue_job(&self.seal_job_tx, move || crate::storage::blob_delete(&content_hash));
                }
            }
        }
    }

    /// Tick drain: a landed (or failed) pigeon answers the operator as a BridgeOut row naming where it landed, so the drop is confirmed in the same transcript that ran the commands.
    pub(super) fn drain_pigeon_landed(&mut self) {
        let mut landed = Vec::new();
        while let Ok(v) = self.pigeon_landed_rx.try_recv() {
            landed.push(v);
        }
        for (contact, res, dir) in landed {
            // The bar has done its job either way — the row that follows says where it landed, or that it did not.
            self.pigeon_progress.retain(|_, pp| pp.got < pp.of);
            let body = match res {
                Some(l) => {
                    crate::logf!("PIGEON: landed {} at {}", l.name, l.path);
                    l.path
                }
                None => {
                    crate::logf!("PIGEON: landing failed in {}", dir);
                    tr(Msg::PigeonLandFailed(&dir)).into_owned()
                }
            };
            // The sibling may have been removed while the pigeon finalized — the log above is then the only record.
            if let Some(ci) = self.ci_of(&contact) {
                self.send_chain_message(ci, &body, false, Some((crate::types::RefKind::BridgeOut, 0)), None);
            }
        }
    }
}

/// The pigeon_ack thinning rule: send on the first chunk, the last, and every time the bar crosses one sixty-fourth — at most ~65 frames per pigeon however large, every one of them for a small pigeon.
pub(super) fn pigeon_ack_due(prev_got: u32, got: u32, of: u32) -> bool {
    if got == 0 || of == 0 {
        return false;
    }
    if got >= of || prev_got == 0 {
        return true;
    }
    (prev_got as u64 * 64 / of as u64) != (got as u64 * 64 / of as u64)
}

#[cfg(test)]
mod ack_thinning {
    use super::pigeon_ack_due;

    /// A large pigeon's acks are bounded; a small one's are every chunk.
    #[test]
    fn at_most_sixty_five_acks_however_large() {
        let count = |of: u32| (1..=of).filter(|&g| pigeon_ack_due(g - 1, g, of)).count();
        assert_eq!(count(1), 1);
        assert_eq!(count(20), 20, "every chunk of a small pigeon");
        assert!(count(545) <= 65, "545 chunks → {} acks", count(545));
        assert!(count(100_000) <= 65);
        assert!(pigeon_ack_due(544, 545, 545), "the last chunk always speaks");
        assert!(pigeon_ack_due(0, 1, 545), "and the first");
        assert!(!pigeon_ack_due(0, 0, 545));
    }
}
