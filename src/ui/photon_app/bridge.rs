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
                    "Bridge ready. Type a command prefixed with \u{201c}$ \u{201d} (e.g. $ uptime) and this device runs it, replying with the output. Requires the target to have the remote-terminal host enabled.".to_string(),
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

    /// Whether this device serves remote shells to fleet siblings (default OFF). Device-scope vault flag (was the `<config>/remote_terminal` marker file); a resident/headless host honours it with no UI.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn remote_terminal_enabled() -> bool {
        crate::storage::device_flag("flags/remote_terminal")
    }

    /// Handle one received `term` DATA frame — CROSS-PLATFORM, the chat-as-shell model (line in, line out; the PTY host in network/bridge.rs is a separate future path). Gate: fold-verified SIBLING only (our own device), never a friend. Two roles:
    /// - HOST (desktop-unix + opt-in): a `$ `-prefixed line is a command → run it + reply with the output as another DATA frame. A non-command line just posts to the sibling conversation.
    /// - CLIENT (any platform): a DATA frame is a reply (command output or a chat line) → post it into that sibling's conversation as an incoming bubble.
    pub(super) fn on_bridge_frame(
        &mut self,
        session_id: [u8; 16],
        kind: u8,
        sealed_payload: Vec<u8>,
        sender_device: [u8; 32],
        sender_addr: std::net::SocketAddr,
    ) {
        use crate::network::fgtw::protocol::term_kind;
        // SIBLING gate (both roles). Never a friend.
        let sib_idx = self
            .contacts
            .iter()
            .position(|c| c.is_sibling && c.knows_device(&sender_device));
        let Some(ci) = sib_idx else {
            crate::log("BRIDGE: frame from a non-sibling — dropped");
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

        // HOST role: a `$ ` command runs here (desktop-unix + opt-in). Remember where to reply.
        #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
        {
            if let Some(cmd) = line.strip_prefix("$ ").or_else(|| line.strip_prefix("$\t")) {
                let relay_to = self
                    .contacts
                    .get(ci)
                    .filter(|c| c.validated_path.is_none())
                    .map(|c| c.relay_device_list())
                    .unwrap_or_default();
                self.bridge_clients
                    .insert(session_id, (sender_device, (sender_addr, None), relay_to));
                if Self::remote_terminal_enabled() {
                    self.run_bridge_command(session_id, cmd);
                } else {
                    // LOUD REFUSAL: a disabled host used to swallow `$ ` commands into a silent chat bubble — the client couldn't tell disabled from broken from unreachable (the Europe incident's shape, and the flag-day census wiped the old marker file fleet-wide with no migration, so EVERY host went silently dark). The command still shows in both histories; the client now gets told exactly what to do about it.
                    crate::logf!("BRIDGE: `$` command from sibling REFUSED — remote terminal is disabled on this host");
                    let notice = "[bridge host disabled — on this machine run: photon-messenger --enable-remote-terminal (while photon is closed), then relaunch photon]";
                    self.send_bridge_frame(session_id, notice.as_bytes());
                }
                // Also show the incoming command in the host's own conversation history.
                self.bridge_post_bubble(ci, &line, false);
                return;
            }
        }
        // Otherwise (CLIENT reply, or a plain chat line): post it as an incoming bubble in that sibling's conversation.
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

    /// BRIDGE client SEND: transmit a line typed in a sibling conversation to that device as a `term` DATA frame (the sibling device-to-device transport). The line rides fleet-sealed; the host runs any `$ ` command and replies with a term DATA frame carrying the output, which our client-receive turns back into a chat bubble. `session_id` is derived from the device pair so both sides agree without a handshake.
    pub(super) fn send_bridge_text(&mut self, ci: usize, text: &str) {
        let Some((device, addr_pair, relay_to)) = self.contacts.get(ci).map(|c| {
            // Always gather a relay list for a sibling with no proven path — the bridge must reach an idle device even when presence never adopted a direct address.
            let relay = if c.validated_path.is_none() {
                c.relay_device_list()
            } else {
                Vec::new()
            };
            (c.public_identity.key, c.race_addrs(), relay)
        }) else {
            return;
        };
        // If there's no direct address, fall back to the RELAY sentinel (0.0.0.0:0) + the relay list — same path chat uses for an unreachable-but-relayable peer. Only truly hopeless (no address AND no relay) bails.
        let (peer_addr, alt_addr) = match addr_pair {
            Some(pair) => pair,
            None if !relay_to.is_empty() => (crate::network::status::RELAY_ADDR, None),
            None => {
                self.ready_toast =
                    Some("That device has no address or relay yet — can't reach it.".to_string());
                self.ready_toast_screen = None;
                crate::logf!(
                    "BRIDGE: send bail — sibling {} has no address and no relay",
                    crate::fp(&device)
                );
                return;
            }
        };
        let Some(fleet_key) = self.fleet_key_cached() else {
            crate::log("BRIDGE: send bail — no fleet key");
            return;
        };
        // Session id = blake3(sorted device pair) truncated — stable, handshake-free, per-pair.
        let session_id = self.bridge_session_id(&device);
        let Ok(sealed) = crate::network::bridge::seal_term(text.as_bytes(), &fleet_key) else {
            return;
        };
        let (Some(kp), Some(checker)) =
            (self.device_keypair.as_ref(), self.status_checker.as_ref())
        else {
            crate::log("BRIDGE: send bail — no device key or status checker");
            return;
        };
        let Ok(vsf_bytes) = crate::network::fgtw::protocol::build_term_vsf(
            &session_id,
            crate::network::fgtw::protocol::term_kind::DATA,
            sealed,
            kp.public.as_bytes(),
            kp.secret.as_bytes(),
        ) else {
            return;
        };
        checker.send_history(crate::network::status::HistorySendRequest {
            peer_addr,
            alt_addr,
            recipient_pubkey: device,
            vsf_bytes,
            relay_to: relay_to.clone(),
        });
        crate::logf!(
            "BRIDGE: sent line to sibling {} via {} ({} relay targets)",
            crate::fp(&device),
            peer_addr,
            relay_to.len()
        );
    }

    /// Stable, handshake-free session id for a device pair: blake3 of the two device pubkeys sorted. Both ends compute the same 16 bytes.
    pub(super) fn bridge_session_id(&self, other_device: &[u8; 32]) -> [u8; 16] {
        let ours = self
            .device_keypair
            .as_ref()
            .map(|kp| *kp.public.as_bytes())
            .unwrap_or([0u8; 32]);
        let (a, b) = if ours <= *other_device {
            (ours, *other_device)
        } else {
            (*other_device, ours)
        };
        let mut input = Vec::with_capacity(64);
        input.extend_from_slice(&a);
        input.extend_from_slice(&b);
        let h = blake3::hash(&input);
        let mut id = [0u8; 16];
        id.copy_from_slice(&h.as_bytes()[..16]);
        id
    }

    /// BRIDGE (chat-as-shell): run one `$ ` command with the login shell, then reply to the client with the combined stdout+stderr as a term DATA frame (which the client posts as a bubble). Output tail-capped; non-zero exit appended. Runs in the operator's own login environment — the operator shelling into their own box.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn run_bridge_command(&mut self, session_id: [u8; 16], cmd: &str) {
        const MAX_OUT: usize = 12 * 1024; // keep a reply comfortably inside one frame
        crate::logf!("BRIDGE: running command from sibling: {}", cmd);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let output = std::process::Command::new(&shell)
            .arg("-lc")
            .arg(cmd)
            .output();
        let reply = match output {
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
                    body = format!("…(truncated {} bytes)\n{}", cut, &body[cut..]);
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
        };
        // Reply to the client as a term DATA frame; also show it in the host's own conversation.
        self.send_bridge_frame(session_id, reply.as_bytes());
        if let Some(ci) = self.contacts.iter().position(|c| {
            self.bridge_clients
                .get(&session_id)
                .map(|(dev, _, _)| c.is_sibling && c.knows_device(dev))
                .unwrap_or(false)
        }) {
            self.bridge_post_bubble(ci, &reply, true);
        }
    }

    /// Send a term DATA frame to the client of `session_id` (host → client reply path).
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    pub(super) fn send_bridge_frame(&self, session_id: [u8; 16], payload: &[u8]) {
        let Some((device, addr_pair, relay_to)) = self.bridge_clients.get(&session_id).cloned()
        else {
            return;
        };
        let Some(fleet_key) = self.fleet_key_cached() else {
            return;
        };
        let Ok(sealed) = crate::network::bridge::seal_term(payload, &fleet_key) else {
            return;
        };
        let (Some(kp), Some(checker)) =
            (self.device_keypair.as_ref(), self.status_checker.as_ref())
        else {
            return;
        };
        let Ok(vsf_bytes) = crate::network::fgtw::protocol::build_term_vsf(
            &session_id,
            crate::network::fgtw::protocol::term_kind::DATA,
            sealed,
            kp.public.as_bytes(),
            kp.secret.as_bytes(),
        ) else {
            return;
        };
        checker.send_history(crate::network::status::HistorySendRequest {
            peer_addr: addr_pair.0,
            alt_addr: addr_pair.1,
            recipient_pubkey: device,
            vsf_bytes,
            relay_to,
        });
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
