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
        // Reply content is the RAW shell output — no sentinel in it. The OUTPUT nature rides the TYPED reference field (RefKind::BridgeOut), so the operator's side renders it as an ordinary bubble and the host-run gate skips it (anti-bounce). Same durable chain send as any message.
        self.send_chain_message(ci, &reply, false, Some((crate::types::RefKind::BridgeOut, 0)));
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
