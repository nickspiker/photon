//! BRIDGE remote terminal + unattended reboot — sibling PTY frames, bridge conversations, command execution, and the reboot-capsule/unattended markers.

use super::*;

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
        // First open with an empty thread: drop a local-only hint so the screen explains itself. Stored as an incoming bubble (not sent) — purely a UI seed.
        if let Some(conv) = self.conv_mut_of(ci) {
            if conv.messages.is_empty() {
                let hint = ChatMessage::new_with_timestamp(
                    "Bridge ready. Type a command prefixed with \u{201c}$ \u{201d} (e.g. $ uptime) and the other device runs it, replying with the output. It's your own fleet — no setup on either end.".to_string(),
                    false,
                    vsf::eagle_time_oscillations(),
                );
                conv.insert_message_sorted(hint);
            }
        }
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

    /// Handle one received `term` DATA frame — CROSS-PLATFORM, the chat-as-shell model (line in, line out; the PTY host in network/bridge.rs is a separate future path). Gate: fold-verified, NON-LOCKED sibling only (our own device), never a friend — and that gate is the WHOLE authorization (no host flag, Nick's ruling 2026-08-21: the bridge is a regular chat screen between your own devices). Two roles:
    /// - HOST (desktop-unix): a `$ `-prefixed line is a command → run it + reply with the output as another DATA frame. A non-command line just posts to the sibling conversation.
    /// - CLIENT (any platform): a DATA frame is a reply (command output or a chat line) → post it into that sibling's conversation as an incoming bubble.
    pub(super) fn on_bridge_frame(
        &mut self,
        // session_id + sender_addr were the term-reply routing keys; the reply rides the durable chain now (run_bridge_command_chat), so only the sender identity + payload matter here.
        _session_id: [u8; 16],
        kind: u8,
        sealed_payload: Vec<u8>,
        sender_device: [u8; 32],
        _sender_addr: std::net::SocketAddr,
    ) {
        use crate::network::fgtw::protocol::term_kind;
        // SIBLING gate (both roles). Never a friend — and never a LOCKED-OUT sibling: with no host flag (Nick's ruling 2026-08-21, the fold IS the authorization), this line is the whole wall between a stolen device and a shell on every fleet machine.
        let sib_idx = self
            .contacts
            .iter()
            .position(|c| c.is_sibling && !c.locked_out && c.knows_device(&sender_device));
        let Some(ci) = sib_idx else {
            crate::log("BRIDGE: frame from a non-sibling (or locked-out) device — dropped");
            return;
        };
        let Some(fleet_key) = self.fleet_key_cached() else {
            crate::log("BRIDGE: no fleet key — cannot open frame");
            return;
        };
        let payload = match crate::network::bridge::open_term(&sealed_payload, &fleet_key) {
            Some(p) => p,
            None => {
                crate::log("BRIDGE: frame failed to open (wrong fleet key / tamper) — dropped");
                return;
            }
        };
        if kind != term_kind::DATA {
            return; // only DATA carries chat-as-shell lines in this model
        }
        let line = String::from_utf8_lossy(&payload).to_string();

        // HOST role: a `$ ` command runs here (desktop-unix). NO host flag (Nick's ruling 2026-08-21): the fold-verified, non-locked sibling gate above IS the authorization — the bridge conversation is a regular chat screen and a `$ ` line just runs, exactly as typing it at that machine would. The old off-by-default flag was the Europe incident's second half: the census wiped its marker fleet-wide and a disabled host swallowed commands into silent chat bubbles.
        // BACKWARD-COMPAT ONLY: this term-frame receive path exists so a not-yet-updated sibling (old build, still term-sending) can still drive this host during a rollout. New builds send commands as ORDINARY chat messages (the durable path); those never reach here — they land in the chat receive path, which runs the same `run_bridge_command_chat`. Both roads lead to one runner + one chat reply.
        #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
        {
            if let Some(cmd) = line.strip_prefix("$ ").or_else(|| line.strip_prefix("$\t")) {
                let cmd = cmd.to_string();
                // Show the incoming command in the host's own conversation, then run + reply over the durable chain (not a term frame).
                self.bridge_post_bubble(ci, &line, false);
                self.run_bridge_command_chat(ci, &cmd);
                return;
            }
        }
        // Otherwise (a plain chat line over the legacy term path): post it as an incoming bubble in that sibling's conversation.
        self.bridge_post_bubble(ci, &line, false);
    }

    /// Post a bridge line into a sibling conversation as a stored message (incoming). Persists so it survives restart.
    pub(super) fn bridge_post_bubble(&mut self, ci: usize, text: &str, outgoing: bool) {
        if let Some(conv) = self.conv_mut_of(ci) {
            let mut msg = ChatMessage::new_with_timestamp(
                text.to_string(),
                outgoing,
                vsf::eagle_time_oscillations(),
            );
            msg.delivered = true;
            conv.insert_message_sorted(msg);
            conv.scroll_offset = 0.0;
        }
        self.persist_messages_async(ci);
    }

    /// Run one `$ ` command with the login shell and return the combined stdout+stderr, tail-capped, non-zero exit appended. Pure — no reply, no bubble; the caller routes the output. Runs in the operator's own login environment (the operator shelling into their own box). Blocks the caller for the command's duration: this runs on the HOST (the remote box, not the operator's screen), so a slow command never freezes the operator's UI — only the unwatched host's.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn execute_bridge_command(cmd: &str) -> String {
        const MAX_OUT: usize = 12 * 1024; // keep a reply comfortably inside one chain message
        crate::logf!("BRIDGE: running command from sibling: {}", cmd);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        match std::process::Command::new(&shell).arg("-lc").arg(cmd).output() {
            Ok(out) => {
                let mut body = String::new();
                body.push_str(&String::from_utf8_lossy(&out.stdout));
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stderr.is_empty() {
                    body.push_str(&stderr);
                }
                if body.len() > MAX_OUT {
                    // Keep the TAIL — the end of a command's output is usually what matters.
                    let cut = body.len() - MAX_OUT;
                    body = format!("\u{2026}(truncated {} bytes)\n{}", cut, &body[cut..]);
                }
                let code = out.status.code().unwrap_or(-1);
                if body.trim().is_empty() {
                    body = if code == 0 {
                        "(no output)".to_string()
                    } else {
                        format!("(no output, exit {})", code)
                    };
                } else if code != 0 {
                    body.push_str(&format!("\n[exit {}]", code));
                }
                body
            }
            Err(e) => format!("(failed to run: {})", e),
        }
    }

    /// HOST role, chat transport (the durable path Nick asked for 2026-08-21): a `$ ` command arrived as an ORDINARY chat message in the sibling `ci`'s conversation — run it and reply with the output as an ordinary chat message BACK to that sibling. The command reached us with full chain reliability (retransmit + ACK + re-serve); the reply rides the same machinery home. The operator sees their command brighten on our ACK (it reached the terminal), then the output land as a normal incoming bubble. No term frames, no session map, no host flag — the fold-verified sibling gate at the receive site is the whole authorization.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn run_bridge_command_chat(&mut self, ci: usize, cmd: &str) {
        let reply = Self::execute_bridge_command(cmd);
        // Reply rides the regular chain send — durable, retransmitted, ACKed, re-served on reconnect, exactly like any message in this conversation.
        self.send_chain_message(ci, &reply, false, None);
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
