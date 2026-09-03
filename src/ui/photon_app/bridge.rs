//! BRIDGE remote terminal + unattended reboot — persistent per-sibling shell, bridge conversations, command execution, and the reboot-capsule/unattended markers.

use super::*;

/// Work item for the off-thread bridge executor: run a command in a sibling's persistent shell, or reset (kill) that sibling's shell so the next command starts fresh.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
pub(super) enum BridgeJob {
    /// (contact idx, device, command text, the command row's eagle_time — the target streamed output frames reference).
    Run(usize, [u8; 32], String, i64),
    Reset([u8; 32]),
}

/// One streamed emission from the executor toward the wire: `body` is the FULL accumulated output so far (a snapshot, never a delta — loss/reorder/dedup of any one frame is then a free no-op), `target` is the command row's eagle_time (what the client's replace-in-place keys on), `fin` carries the exit code once the command completed, and the locus names where the shell stands so the operator is never blind to host+cwd again (field 2026-08-23: a pull meant for photon ran in keys/). Partials ride a latest-wins slot (a superseded snapshot is garbage by definition); finals ride the ordered channel because every one must reach the wire.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
pub(super) struct BridgeEmit {
    pub ci: usize,
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

/// Keep the LAST `cap` bytes of `s` (char-boundary-safe), prefixed with an elision note naming what stayed behind.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
fn bridge_cap_tail(s: &str, cap: usize) -> String {
    if s.len() <= cap {
        return s.to_string();
    }
    let mut start = s.len() - cap;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    tr(Msg::BridgeElided { bytes: start, output: &s[start..] }).into_owned()
}

#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
fn bridge_wake(w: &Option<std::sync::Arc<dyn WakeSender<PhotonEvent>>>) {
    if let Some(w) = w {
        let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
    }
}

/// One worker thread per sibling device, owning that device's persistent shell. Per-device because the executor used to serialize EVERY sibling thru one thread — with no timeout, one long build would have queued every other bridge behind it. The worker blocks in run_streaming for as long as the command takes; liveness is visible thru the streamed partials, and the operator's stop lever runs thru BridgeFgMap, not thru this queue.
#[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
fn spawn_bridge_worker(
    dev: [u8; 32],
    partials: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<usize, BridgeEmit>>>,
    fg: BridgeFgMap,
    wake: Option<std::sync::Arc<dyn WakeSender<PhotonEvent>>>,
) -> std::sync::mpsc::Sender<(usize, String, i64)> {
    // Append `chunk` to the command's unsent-delta buffer (creating it on first output), bounding memory by trimming the FRONT with an explicit dropped-byte count. Wake only on the empty→occupied edge so a spewing build can't flood the event loop — the UI drain reads the buffer at its own pace.
    fn push_delta(
        partials: &std::sync::Mutex<std::collections::HashMap<usize, BridgeEmit>>,
        ci: usize,
        ts: i64,
        seq: u64,
        chunk: &str,
        fin: Option<i32>,
        host: &str,
        cwd: &str,
    ) -> bool {
        let mut m = partials.lock().unwrap();
        let e = m.entry(ci).or_insert_with(|| BridgeEmit { ci, target: ts, seq, body: String::new(), dropped: 0, fin: None, host: host.to_string(), cwd: cwd.to_string() });
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
    let (tx, rx) = std::sync::mpsc::channel::<(usize, String, i64)>();
    let spawned = std::thread::Builder::new()
        .name("bridge-shell".to_string())
        .spawn(move || {
            let mut shell: Option<BridgeShell> = None;
            let mut last_cwd = String::new();
            while let Ok((ci, cmd, ts)) = rx.recv() {
                if shell.is_none() {
                    match BridgeShell::spawn() {
                        Ok(s) => {
                            fg.lock().unwrap().insert(dev, s.child.id() as i32);
                            shell = Some(s);
                        }
                        Err(e) => {
                            push_delta(&partials, ci, ts, 1, &tr(Msg::BridgeShellStartFailed(&e.to_string())), Some(-1), "", "");
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
                    if push_delta(&partials, ci, ts, seq, chunk, None, &host, &cwd0) {
                        bridge_wake(&wake);
                    }
                });
                match res {
                    Ok((code, cwd, _)) => {
                        last_cwd = cwd.clone();
                        // "Finished" is a FIELD, not a message (Nick 2026-09-03): the exit code folds into whatever delta is still buffered and rides out on that frame. A command that never printed and failed still names itself; clean silent success stays an empty-bodied exit frame the client stamps without a bubble.
                        let text = if !emitted_any && code != 0 { tr(Msg::BridgeNoOutput(code)).into_owned() } else { String::new() };
                        push_delta(&partials, ci, ts, seq + 1, &text, Some(code), &host, &cwd);
                        bridge_wake(&wake);
                    }
                    Err(e) => {
                        // Registry absence = a deliberate Reset killed us — the client wiped its screen, so a death notice would land as a stray bubble in a fresh session. A REAL death (bash exited, crashed) reports once and the next command respawns.
                        let was_registered = fg.lock().unwrap().remove(&dev).is_some();
                        if was_registered {
                            push_delta(&partials, ci, ts, seq + 1, &tr(Msg::BridgeShellDied(&e)), Some(-1), &host, &cwd0);
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
        let Some(dev) = self.contacts.get(ci).and_then(|c| c.device_key()) else {
            return;
        };
        self.ensure_bridge_exec();
        if let Some(tx) = self.bridge_cmd_tx.as_ref() {
            let _ = tx.send(BridgeJob::Run(ci, dev, cmd.to_string(), cmd_ts));
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
            let wire = crate::network::message_package::BridgeWire {
                exit: Some(-1),
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
            std::sync::Mutex<std::collections::HashMap<usize, BridgeEmit>>,
        > = Default::default();
        let fg: BridgeFgMap = Default::default();
        self.bridge_partials = Some(partials.clone());
        self.bridge_fg = Some(fg.clone());
        std::thread::Builder::new()
            .name("bridge-exec".to_string())
            .spawn(move || {
                let mut workers: std::collections::HashMap<
                    [u8; 32],
                    std::sync::mpsc::Sender<(usize, String, i64)>,
                > = std::collections::HashMap::new();
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
                        BridgeJob::Run(ci, dev, cmd, ts) => {
                            crate::logf!("BRIDGE: running command from sibling: {}", cmd);
                            let alive = workers
                                .get(&dev)
                                .map(|tx| tx.send((ci, cmd.clone(), ts)).is_ok())
                                .unwrap_or(false);
                            if !alive {
                                let tx = spawn_bridge_worker(
                                    dev,
                                    partials.clone(),
                                    fg.clone(),
                                    wake.clone(),
                                );
                                let _ = tx.send((ci, cmd, ts));
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
        let put_back = |slots: &std::sync::Mutex<std::collections::HashMap<usize, BridgeEmit>>, e: BridgeEmit| {
            let mut m = slots.lock().unwrap();
            match m.entry(e.ci) {
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
            let is_final = e.fin.is_some();
            if !is_final {
                // THE ONE TIMER (Nick's grant, 2026-08-31): deltas reach the wire at most once per second per conversation — the spool collapses bursts, this paces the broadcast. The exit-carrying frame is never paced.
                let recently = self
                    .bridge_partial_sent
                    .get(&e.ci)
                    .map_or(false, |t| t.elapsed() < std::time::Duration::from_secs(1));
                // ONE delta in flight per feed, gated on ITS OWN ACK edge — the whole-lane pending count starved the feed behind fleet-sync chatter (the silent v82 deploy, 2026-09-03). The ACK arriving is the wake edge that ships the next spool; a parked spool keeps accumulating, nothing is lost.
                let prev_unacked = self.bridge_partial_inflight.get(&e.ci).map_or(false, |&et| {
                    self.contacts
                        .get(e.ci)
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
            };
            if is_final {
                // The exit-carrying delta rides the full durable path (host row + retransmit + held-row re-serve) — it is the one frame that must survive.
                self.bridge_partial_inflight.remove(&e.ci);
                self.send_chain_message(
                    e.ci,
                    &body,
                    false,
                    Some((crate::types::RefKind::BridgeOut, e.target)),
                    Some(wire),
                );
            } else {
                // Mid-command deltas ride chain_transmit directly with an eagle_time minted HERE so the own-ACK gate can watch this exact frame leave pending. A refused send (window full, no address yet) puts the spool back intact — the transcript never loses a byte to flow control.
                let et = vsf::eagle_time_oscillations();
                if self.chain_transmit(e.ci, &body, et, Some((crate::types::RefKind::BridgeOut, e.target)), Some(&wire)) {
                    self.bridge_partial_inflight.insert(e.ci, et);
                    self.bridge_partial_sent.insert(e.ci, std::time::Instant::now());
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
