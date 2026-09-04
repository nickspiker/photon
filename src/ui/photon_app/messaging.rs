//! The message send path — compose submit, chain transmit, persistence, held/pending resend, and the chain probe/seal that retires a completed ceremony.

use super::*;

impl PhotonApp {
    /// Textbox front-end for the open conversation: pull + trim the compose text, hand it to [`Self::send_chain_message`] for the active contact (bubble shown), then clear the box.
    pub(super) fn submit_message(&mut self) {
        let Some(ci) = self.active_contact() else {
            return;
        };
        // Pull the compose text and send it VERBATIM. Any non-empty content sends, whitespace included (a lone space, all spaces, a newline are all valid messages). No trim, no whitespace judgment.
        let text: String = match self.message_textbox.as_ref() {
            Some(tb) => tb.chars.iter().collect(),
            None => return,
        };
        // THE PROMPT GATE (Nick 2026-08-26): a terminal doesn't take the next command until the prompt returns. While a bridge command is in flight (no final yet), refuse the send and KEEP the typed text — type-ahead composes, Enter waits. The gate can't wedge: the final clears it, and when a final can never come the stream-loss stamp or Stop's no-op answer does (both stamp bridge_exit). Stop itself is a control row, never gated.
        if !text.is_empty() && self.contacts.get(ci).map_or(false, |c| c.is_sibling) {
            if let Some(t) = self.bridge_inflight_target(ci) {
                // NAME THE HOSTAGE (field 2026-08-28, the gate that held after a displayed final): which command row holds the prompt, and what evidence exists about its final — an anonymous refusal made this class undiagnosable from logs.
                let (has_out_row, out_exit) = self
                    .conv_of(ci)
                    .and_then(|conv| {
                        conv.messages
                            .iter()
                            .find(|m| {
                                m.reference
                                    == Some((crate::types::RefKind::BridgeOut, t))
                            })
                            .map(|m| (true, m.bridge_exit))
                    })
                    .unwrap_or((false, None));
                let exit_s = out_exit.map(|e| e.to_string()).unwrap_or_else(|| "-".into());
                crate::logf!(
                    "BRIDGE: send held — command eagle_time {} is delivered with no final (output row: {}, exit: {}); Stop to interrupt",
                    t,
                    if has_out_row { "present" } else { "none" },
                    exit_s
                );
                self.scene_dirty = true;
                return;
            }
        }
        if text.is_empty() {
            // Empty while a reply/edit/react is armed = cancel the arm, nothing sends (an "empty edit" is a delete's job, and probing from an armed state would be a surprise ping).
            if self.compose_reply_to.take().is_some()
                | self.compose_edit_of.take().is_some()
                | self.compose_react_to.take().is_some()
            {
                self.scene_dirty = true;
                return;
            }
            // Empty send = liveness probe. Optimistically mark the peer offline and ping them; a returning pong flips is_online back true (check_status_updates), so an empty send confirms whether they're actually reachable right now instead of doing nothing.
            self.contacts[ci].is_online = false;
            self.ping_contact(ci);
            return;
        }
        // An armed custom reaction takes the typed text (capped short — it's a reaction, not a message) as the glyph.
        if let Some(target) = self.compose_react_to.take() {
            let glyph: String = text.chars().take(8).collect();
            if self.send_chain_message(
                ci,
                &glyph,
                false,
                Some((crate::types::RefKind::React, target)),
                None,
            ) {
                self.stamp_react_used(&glyph);
            }
            if let Some(tb) = self.message_textbox.as_mut() {
                tb.clear();
            }
            self.pending_input_reset = true;
            self.scene_dirty = true;
            return;
        }
        // An armed edit/reply rides as the TYPED reference; the text goes as-is — the row then takes every path (chain, ACK, fleet sync, pages, digest) as an ordinary message and the reference resolves at render.
        let reference = self
            .compose_edit_of
            .take()
            .map(|t| (crate::types::RefKind::Edit, t))
            .or_else(|| {
                self.compose_reply_to
                    .take()
                    .map(|t| (crate::types::RefKind::Reply, t))
            })
            .or_else(|| {
                // A sibling conversation IS the bridge terminal, and a command is marked by TYPE, never sniffed from content (the 2026-08-23 rule): the host runs exactly the rows carrying BridgeCmd.
                self.contacts
                    .get(ci)
                    .filter(|c| c.is_sibling)
                    .map(|_| (crate::types::RefKind::BridgeCmd, 0))
            });
        self.send_chain_message(ci, &text, false, reference, None);
        if let Some(tb) = self.message_textbox.as_mut() {
            tb.clear();
        }
        // Tell the Android host to restart IME input — a predictive keyboard still holds the just-sent text as a composing buffer and would re-materialise it on the next keystroke without this.
        self.pending_input_reset = true;
    }

    /// Encrypt + send + persist one chat message to `contact_idx` over the friendship chain, appending an outgoing bubble only when `!suppress_bubble`. Returns `true` if the message was dispatched to the network (so callers like the chain-weave probe only latch `probe_sent` on an actual send, and retry next cycle if the contact had no address yet). This is the reusable core factored out of the old open-contact send: it works for ANY contact index (not just `active_contact`), so the hidden chain-weave probe can ride the exact same ratchet path with its UI suppressed. Chain math (`prepare_send`, salt/advance) is untouched — the probe is a normal message whose only difference is a reserved marker content and a hidden bubble.
    pub(super) fn send_chain_message(
        &mut self,
        contact_idx: usize,
        text: &str,
        suppress_bubble: bool,
        reference: Option<(crate::types::RefKind, i64)>,
        bridge: Option<crate::network::message_package::BridgeWire>,
    ) -> bool {
        let ci = contact_idx;
        let text = text.to_string();

        // BRIDGE rides the REGULAR chain path now (Nick's call 2026-08-21): a `$ ` command typed in a sibling conversation is an ordinary message to that one device — a per-sibling conversation is exactly [our_sibling_pid, that_device_pid], so the general path below already targets the single device with full chain durability (retransmit + ACK + re-serve). The old is_sibling branch fire-and-forgot a term frame with NONE of that, which is why commands to a momentarily-unreachable device evaporated. The host detects the `$ ` prefix on RECEIVE (conversation.rs) and replies with another ordinary message; faint-until-ACKed rendering comes free.

        // How many people does this message have to reach? For our own notes the answer is zero, so there is nothing to encrypt, nothing to dispatch, and nothing left to wait for — the row is delivered because delivery to an empty set is already complete. That is not a special case for self; it is what the general rule evaluates to when the participant set has one member (docs: the conversation model). A conversation with one remote takes the chain path below, and so does one with fifty.
        let remotes = match self.contacts.get(ci) {
            // `our_party_id` already knows which id space this conversation lives in — the identity pubkey for friends and our own notes, the device-derived pid for a fleet sibling — so the participant set is built from the right values without asking what kind of row this is.
            Some(c) => match self.our_party_id(c) {
                Some(us) => c.remote_count(&us),
                None => return false,
            },
            None => return false,
        };
        // A reaction/edit targets an EXISTING row — inserting its referencing row must not yank the view to the bottom (the target the user is looking at may be far up the stream).
        let is_quiet_row = matches!(
            reference,
            Some((
                crate::types::RefKind::Edit | crate::types::RefKind::React,
                _
            ))
        );
        if remotes == 0 {
            // WRITE-CONFIRM-THEN-SEND holds here too (2026-08-21 erasure ticket): with zero remotes the VAULT is the recipient, so the disk write IS the delivery. The row is born faint like every other send; the writer's durable verdict (drain_persist_done) flips it bright AND releases the sibling push — the old shape set delivered=true and pushed before any write, so a refused persist left a bright RAM ghost that vanished at relaunch while siblings held a copy this device never durably owned.
            let mut msg =
                // CORRECTED time, not the system clock: this stamp is the row's identity AND its sort key on every device that will ever hold it (see network::time_base). The system clock may be deliberately wrong — that is the human's business, not the conversation's.
                ChatMessage::new_with_timestamp(text, true, crate::network::time_base::stamp_osc());
            msg.reference = reference;
            let ts = msg.timestamp;
            let Some(conv) = self.conv_mut_of(ci) else {
                return false;
            };
            conv.insert_message_sorted(msg);
            if !is_quiet_row {
                conv.scroll_offset = 0.0;
            }
            self.persist_messages_signalled(ci, vec![ts]);
            return true;
        }

        let eagle_time = crate::network::time_base::stamp_osc();
        // A suppressed send (the hidden chain-weave probe) shows no UI — wire half only.
        if suppress_bubble {
            return self.chain_transmit(ci, &text, eagle_time, reference, bridge.as_ref());
        }

        // BUBBLE FIRST, WIRE SECOND. The pending-grey bubble appears the instant the user hits send — chain_transmit does weave selection, braid advance, chains persist and PT dispatch, and running it first meant the message rendered as NOTHING for that whole stretch, then grey, then white. The user's mental model (grey immediately, everything else follows) is also the honest one: the row exists the moment they authored it; the wire is delivery, not existence.
        let mut msg = ChatMessage::new_with_timestamp(text.clone(), true, eagle_time);
        msg.reference = reference;
        // The row carries its OWN wire truth (the stop-hang conviction, 2026-08-30): a re-serve rebuilds frames from this row, and a BridgeOut final rebuilt without its seq/exit delivers text the client's gate can never release on ("output row: present, exit: -" — the prompt held forever). Stamp them here so bridge_wire_for_row can resurrect the wire at any re-serve site.
        if let Some(bw) = bridge.as_ref() {
            msg.bridge_seq = bw.seq.unwrap_or(0);
            msg.bridge_exit = bw.exit;
        }
        if let Some(conv) = self.conv_mut_of(ci) {
            conv.insert_message_sorted(msg.clone());
            if !is_quiet_row {
                conv.scroll_offset = 0.0;
            }
        }
        self.persist_messages_async(ci);

        // The wire half is DEFERRED to the next tick (drain_pending_chain_sends): the bubble above can only reach the screen when this handler returns, so running chain_transmit inline here — crypto, chains persist, dispatch — held the frame hostage for its whole duration. Queue it and let the grey bubble present first.
        self.pending_chain_sends
            .push((ci, text, eagle_time, reference, bridge, self.tick_serial));
        return true;
    }

    /// Persist a conversation's message table WITHOUT blocking the UI thread — the no-signal wrapper for saves nothing waits on.
    pub(super) fn persist_messages_async(&mut self, ci: usize) {
        self.persist_messages_signalled(ci, Vec::new());
    }

    /// Persist a conversation's message table WITHOUT blocking the UI thread. Snapshots the conversation and hands it to one background writer that coalesces bursts (latest snapshot per conversation id wins — an older snapshot can never clobber a newer one because the drain keeps only the last). The write itself is the same `save_messages` full-table rewrite; only the thread changed. `signal_rows` names rows whose bright flip + sibling push WAIT on this write (the zero-remote path): their verdict rides back over `persist_done`, and every early exit below answers it immediately — a row waiting on a write that never starts must fail LOUDLY, not sit faint forever.
    pub(super) fn persist_messages_signalled(&mut self, ci: usize, signal_rows: Vec<i64>) {
        // BRIDGE rows are EPHEMERAL — the terminal keeps NO history at rest. Safe now because sibling frames are anchor-only (see the braid selection in send_chain_message): nothing weaves against a bridge row, so not persisting it can't produce a strand miss (the earlier ephemeral attempt DID break the braid precisely because frames still wove strands — field 2026-08-22, one reply resent 12× and never ACKed). Chain durability is untouched: lane positions, pending, and last_received_times live in friendship_chains (persisted separately), so retransmit/ACK/dedup and the is_new_row replay guard all still hold; only the display rows are transient.
        if self.contacts.get(ci).map_or(false, |c| c.is_sibling) {
            // Only the zero-remote branch passes signal rows and a self contact is never a sibling — this line firing means a caller broke that invariant, and its rows will sit faint.
            if !signal_rows.is_empty() {
                crate::logf!("STORAGE: signalled persist on a SIBLING conversation ({} row(s)) — bridge rows are ephemeral by policy; rows stay faint", signal_rows.len());
            }
            return;
        }
        // LATE HYDRATION RESCUE: an un-hydrated conversation (the boot loader hit a load error, or the vault opened after materialize) re-attempts the load here and merges the disk rows UNDER the RAM rows via the (timestamp, content) collapse — a transient vault error costs one retry, not a whole session of refused persists whose RAM rows would then die at relaunch.
        if self.conv_of(ci).is_some_and(|v| !v.hydrated) {
            if let Some(storage) = self.storage.as_ref().cloned() {
                if let Some(conv) = self.conv_mut_of(ci) {
                    if !conv.hydrated {
                        let mut fresh =
                            crate::types::Conversation::new(conv.participants().iter().copied());
                        if crate::storage::contacts::load_messages(&mut fresh, &storage).is_ok() {
                            let disk_rows = fresh.messages.len();
                            for m in conv.messages.drain(..) {
                                fresh.insert_message_sorted(m);
                            }
                            conv.messages = std::mem::take(&mut fresh.messages);
                            conv.hydrated = true;
                            crate::logf!(
                                "STORAGE: late hydration merged {} disk row(s) under the RAM rows for conversation {}",
                                disk_rows,
                                hex::encode(&conv.id().as_bytes()[..4])
                            );
                        }
                    }
                }
            }
        }
        // EVERY exit below is LOUD (field 2026-08-21: a run's later sends vanished at relaunch while earlier ones persisted — three silent failure modes on the one path whose failure IS data loss, and the log couldn't say which fired).
        let Some(conv) = self.conv_of(ci).cloned() else {
            crate::logf!("STORAGE: message persist SKIPPED — no conversation object resolves for contact index {} (rows live in RAM only until this is fixed)", ci);
            if !signal_rows.is_empty() {
                let _ = self.persist_done.0.send(MessagesDurableVerdict {
                    conv_id: crate::types::friendship::FriendshipId([0u8; 32]),
                    rows: signal_rows,
                    err: Some("no conversation object resolves".to_string()),
                });
            }
            return;
        };
        // HYDRATION GATE (the 2026-08-21 relaunch erasure): a conversation that never successfully loaded its durable table holds a partial (often empty) row set, and persisting that snapshot re-puts only what RAM has — which the legacy sweep then treated as the whole table. Refuse until load_messages has succeeded for this conversation; the rows stay in RAM and the next hydrated persist carries them.
        if !conv.hydrated {
            crate::logf!("STORAGE: message persist REFUSED for conversation {} — never hydrated from the vault (a write now could shadow durable rows); rows stay in RAM", hex::encode(&conv.id().as_bytes()[..4]));
            if !signal_rows.is_empty() {
                let _ = self.persist_done.0.send(MessagesDurableVerdict {
                    conv_id: conv.id(),
                    rows: signal_rows,
                    err: Some("conversation not hydrated".to_string()),
                });
            }
            return;
        }
        let Some(storage) = self.storage.as_ref().cloned() else {
            crate::log("STORAGE: message persist SKIPPED — no storage (vault not open)");
            if !signal_rows.is_empty() {
                let _ = self.persist_done.0.send(MessagesDurableVerdict {
                    conv_id: conv.id(),
                    rows: signal_rows,
                    err: Some("vault not open".to_string()),
                });
            }
            return;
        };
        // Storage rides WITH each snapshot (not captured once): logout/login swaps the vault, and a worker holding the first session's Arc would write a dead vault forever.
        type PersistItem = (
            crate::types::Conversation,
            std::sync::Arc<crate::storage::FlatStorage>,
            Vec<i64>,
        );
        let done_tx = self.persist_done.0.clone();
        // The verdict must WAKE the loop, not just queue: on an idle app nothing else pumps events, so a posted verdict sat undrained until an unrelated ping or keystroke — the field's "self-notes stick or take forever to go bright" (2026-08-25). The bridge executor learned this edge first; same law here.
        let wake = self.event_proxy.clone();
        let pending = self.durable_pending.clone();
        let tx = self.persist_tx.get_or_insert_with(|| {
            let (tx, rx) = std::sync::mpsc::channel::<PersistItem>();
            std::thread::spawn(move || {
                while let Ok(first) = rx.recv() {
                    // Coalesce the burst: keep the newest snapshot per conversation — and CARRY the superseded snapshot's signal rows onto its replacement (the newer snapshot contains those rows too, so its durable write answers for them; same law as the chains writer).
                    let mut consumed = 1usize;
                    let mut latest: Vec<PersistItem> = vec![first];
                    while let Ok(next) = rx.try_recv() {
                        consumed += 1;
                        let mut carried: Vec<i64> = Vec::new();
                        latest.retain_mut(|(v, _, rows)| {
                            if v.id() == next.0.id() {
                                carried.append(rows);
                                false
                            } else {
                                true
                            }
                        });
                        let (v, st, mut rows) = next;
                        carried.append(&mut rows);
                        latest.push((v, st, carried));
                    }
                    for (v, st, rows) in latest {
                        // catch_unwind: ONE poisoned snapshot must not kill the writer for the whole session — a dead writer silently stranded every subsequent message in RAM (they vanished at relaunch).
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            crate::storage::contacts::save_messages(&v, &st)
                        }));
                        let err = match r {
                            Ok(Ok(_)) => None,
                            Ok(Err(e)) => {
                                let m = format!("{e}");
                                crate::logf!("STORAGE: async message persist failed: {}", m);
                                crate::storage::flag_vault_sick();
                                Some(m)
                            }
                            Err(_) => {
                                crate::log("STORAGE: message persist writer caught a PANIC — snapshot dropped, writer survives");
                                Some("writer panicked".to_string())
                            }
                        };
                        if !rows.is_empty() {
                            let _ = done_tx.send(MessagesDurableVerdict {
                                conv_id: v.id(),
                                rows,
                                err,
                            });
                            if let Some(w) = wake.as_ref() {
                                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                            }
                        }
                    }
                    // Quit-drain accounting: every consumed item's write has landed (or errored LOUDLY thru the verdict) — release the quit edge. Decrement AFTER the writes, never on dequeue: the whole point is that the process may not exit while a snapshot is queued OR mid-write (the 2026-09-02 vanish was exactly a quit racing this thread).
                    let (n, cv) = &*pending;
                    let mut n = n.lock().unwrap();
                    *n = n.saturating_sub(consumed);
                    cv.notify_all();
                }
            });
            tx
        });
        if tx
            .send((conv.clone(), storage.clone(), signal_rows.clone()))
            .is_err()
        {
            // The writer thread is dead. Respawn and retry once — the fresh channel's receiver is alive by construction, so the retry cannot loop.
            crate::log("STORAGE: message persist writer was DEAD — respawning and retrying");
            self.persist_tx = None;
            self.persist_messages_signalled(ci, signal_rows);
        } else {
            // Enqueue accounting for the quit drain (increment only on a SENT item — the respawn path above re-enters and counts its own retry).
            *self.durable_pending.0.lock().unwrap() += 1;
        }
    }

    /// Block the quit edge until every queued message/chains durable write has landed. The 2026-09-02 vanish: a deliberate quit exits the process with a snapshot still in the writer queue — the typed row was never written and dies silently (the field's "important message wiped twice"). This is an EDGE wait (condvar signalled by the writers), not a timer: a healthy vault drains in milliseconds; a wedged vault holds the quit visibly (with a log saying what it waits for), which is the honest alternative to eating rows.
    pub(super) fn drain_durable_writers(&self) {
        let (lock, cv) = &*self.durable_pending;
        let mut n = lock.lock().unwrap();
        if *n == 0 {
            return;
        }
        crate::logf!(
            "EXIT: draining {} queued durable write(s) before exit — quitting now would lose them",
            *n
        );
        while *n > 0 {
            n = cv.wait(n).unwrap();
        }
        crate::log("EXIT: durable writers drained — rows are on disk, safe to exit");
    }

    /// Apply the message writer's durable verdicts — the zero-remote commit point. The disk confirmed → the named rows flip bright, their sibling push releases, and the bright state re-persists (the ACK arm's exact discipline). The disk refused → the rows stay faint and the failure surfaces as a TOAST, not a log line (2026-08-21 erasure ticket: the silent version of this failure was indistinguishable from success until relaunch ate the rows; the faint row + resend pill are the honest state and the retry).
    pub(super) fn drain_persist_done(&mut self) {
        while let Ok(verdict) = self.persist_done.1.try_recv() {
            if let Some(err) = verdict.err {
                crate::logf!("STORAGE CRITICAL: durable write refused for {} self row(s) — staying faint, sibling push withheld: {}", verdict.rows.len(), err);
                self.ready_toast = Some(tr(Msg::MessageNotSaved).into_owned());
                self.ready_toast_screen = None;
                self.scene_dirty = true;
                continue;
            }
            let Some(conv) = self
                .conversations
                .iter_mut()
                .find(|v| v.id() == verdict.conv_id)
            else {
                continue;
            };
            let mut bright: Vec<ChatMessage> = Vec::new();
            for ts in &verdict.rows {
                if let Some(m) = conv.messages.iter_mut().find(|m| m.timestamp == *ts) {
                    if !m.delivered {
                        m.delivered = true;
                        bright.push(m.clone());
                    }
                }
            }
            if bright.is_empty() {
                continue;
            }
            self.scene_dirty = true;
            let ci = (0..self.contacts.len()).find(|&i| {
                let c = &self.contacts[i];
                self.our_party_id(c)
                    .is_some_and(|us| c.conversation(&us).id() == verdict.conv_id)
            });
            if let Some(ci) = ci {
                // The sibling push releases HERE, after durability — a copy this device never durably owned must never replicate out (the fleet-amplification half of the erasure).
                self.push_rows_to_siblings(ci, &bright, None);
                // Record the bright state — un-signalled: nothing waits on the flag write, and a lost one re-converges at the next ACK-shaped persist.
                self.persist_messages_async(ci);
            }
        }
    }

    /// Persist a friendship's chains OFF the UI thread with no gated signal — the safe-to-delay saves (ACK pending-removal, chain-sync adopt), where a lost write just re-converges idempotently.
    pub(super) fn persist_chains_async(&mut self, fid: &crate::types::friendship::FriendshipId) {
        let Some((_, chains)) = self.friendship_chains.iter().find(|(id, _)| id == fid) else {
            return;
        };
        let chains = chains.clone();
        self.persist_chains_then(chains, Vec::new());
    }

    /// Persist a chains snapshot OFF the UI thread, coalescing a burst to the newest snapshot per friendship id, then fire the attached signals — the commit-point path (receive's ACK, send's transmit) with persist-before-signal intact. A write failure withholds every attached signal, loudly: no ACK for an unwritten receive (the sender retransmits), no transmit for an unwritten send (the reload re-sends from the persisted tip).
    pub(super) fn persist_chains_then(
        &mut self,
        chains: crate::types::friendship::FriendshipChains,
        actions: Vec<ChainsPostDurable>,
    ) {
        let Some(storage) = self.storage.as_ref().cloned() else {
            return;
        };
        type ChainsItem = (
            crate::types::friendship::FriendshipChains,
            std::sync::Arc<crate::storage::FlatStorage>,
            Vec<ChainsPostDurable>,
        );
        let pending = self.durable_pending.clone();
        let tx = self.chains_persist_tx.get_or_insert_with(|| {
            let (tx, rx) = std::sync::mpsc::channel::<ChainsItem>();
            std::thread::spawn(move || {
                while let Ok(first) = rx.recv() {
                    // Coalesce the burst: keep the newest snapshot per friendship id — and CARRY the superseded snapshot's signals into its replacement. The newest snapshot contains every advance the older one did, so firing them after the newer write keeps persist-before-signal true for all of them.
                    let mut consumed = 1usize;
                    let mut latest: Vec<ChainsItem> = vec![first];
                    while let Ok(next) = rx.try_recv() {
                        consumed += 1;
                        let mut carried: Vec<ChainsPostDurable> = Vec::new();
                        latest.retain_mut(|(c, _, acts)| {
                            if c.id() == next.0.id() {
                                carried.append(acts);
                                false
                            } else {
                                true
                            }
                        });
                        let (c, st, mut acts) = next;
                        carried.append(&mut acts);
                        latest.push((c, st, carried));
                    }
                    for (c, st, acts) in latest {
                        match crate::storage::friendship::save_friendship_chains(&c, &st) {
                            Ok(()) => {
                                for a in acts {
                                    a.fire();
                                }
                            }
                            Err(e) => {
                                crate::logf!("STORAGE CRITICAL: chains persist failed — withholding {} gated signal(s) (ACK/transmit): {}", acts.len(), e);
                                crate::storage::flag_vault_sick();
                            }
                        }
                    }
                    // Quit-drain accounting — same law as the message writer: decrement after the writes land, then release the quit edge.
                    let (n, cv) = &*pending;
                    let mut n = n.lock().unwrap();
                    *n = n.saturating_sub(consumed);
                    cv.notify_all();
                }
            });
            tx
        });
        if tx.send((chains, storage, actions)).is_ok() {
            // Enqueue accounting for the quit drain: a queued chains commit (lane advance / ACK gate) must not die with a quitting process any more than a message row may.
            *self.durable_pending.0.lock().unwrap() += 1;
        }
    }

    /// Persist a conversation's durable bits (unread + history cursor) OFF the UI thread, coalesced to the newest record per conversation. Same worker discipline as the message/chains writers: storage rides with each item so a vault swap can't strand the thread on a dead session.
    pub(super) fn persist_conv_state_async(&mut self, conv_pos: usize) {
        let Some(conv) = self.conversations.get(conv_pos) else {
            return;
        };
        let (addr, buf) = crate::storage::contacts::conversation_state_record(conv);
        let Some(storage) = self.storage.as_ref().cloned() else {
            return;
        };
        type ConvStateItem = (
            [u8; 32],
            [u8; 13],
            std::sync::Arc<crate::storage::FlatStorage>,
        );
        let tx = self.conv_state_persist_tx.get_or_insert_with(|| {
            let (tx, rx) = std::sync::mpsc::channel::<ConvStateItem>();
            std::thread::spawn(move || {
                while let Ok(first) = rx.recv() {
                    // Coalesce the burst: keep the newest record per address.
                    let mut latest: Vec<ConvStateItem> = vec![first];
                    while let Ok(next) = rx.try_recv() {
                        latest.retain(|(a, _, _)| *a != next.0);
                        latest.push(next);
                    }
                    for (a, b, st) in latest {
                        if let Err(e) = st.write_addr(&a, &b) {
                            crate::logf!("STORAGE: async conv-state persist failed: {}", e);
                            crate::storage::flag_vault_sick();
                        }
                    }
                }
            });
            tx
        });
        let _ = tx.send((addr, buf, storage));
    }

    /// Commit finished send encrypts: CAS-advance the lane (prepare_send_commit), then ride the durable-transmit writer — the frame leaves only after the chains land on disk (the C2 law). A CAS miss (an era adopt landed mid-encrypt) voids the ciphertext and re-fires the held row at the fresh position. Always clears the per-friendship encrypt gate, success or not.
    pub(super) fn drain_braid_tx(&mut self) {
        while let Ok(done) = self.braid_tx_rx.try_recv() {
            self.send_encrypt_busy.remove(done.friendship_id.as_bytes());
            let contact_idx = self
                .contacts
                .iter()
                .position(|c| c.friendship_id == Some(done.friendship_id));
            let Some(wire) = done.result else {
                crate::log("CHAT: braid encrypt failed off-thread (lane vanished mid-flight) — row stays held for the sweep");
                continue;
            };
            let committed = self
                .friendship_chains
                .iter_mut()
                .find(|(id, _)| *id == done.friendship_id)
                .map(|(_, chains)| {
                    chains.prepare_send_commit(
                        &wire.lane,
                        &wire.expected_key,
                        wire.ciphertext.clone(),
                        wire.prev_msg_hp,
                        wire.msg_hp,
                        wire.plaintext_hash,
                        done.salt_text.clone(),
                        done.eagle_time,
                        done.woven_strands.clone(),
                    )
                })
                .unwrap_or(false);
            if !committed {
                crate::log("CHAT: send commit CAS voided — the lane moved while encrypting (era adopt); the held-row sweep re-encrypts at the fresh position");
                if let Some(ci) = contact_idx {
                    self.resend_held_messages(ci);
                }
                continue;
            }
            // CALL basket capture (docs/calls.md): the offer's send COMMIT is where the CALLER sees the lane key its offer sealed under — the basket's doomed egg (the callee captures the same value at decrypt, pre-advance). Matched by content: salt_text IS the row text.
            if let Ok(text) = std::str::from_utf8(&done.salt_text) {
                if let Some(sig @ crate::call::signal::CallSignal::Offer { call_id, .. }) =
                    crate::call::signal::CallSignal::parse(text)
                {
                    let mut captured = false;
                    if let Some(call) = self.active_call.as_mut() {
                        if call.call_id == call_id && call.offer_lane_key.is_none() {
                            call.offer_lane_key = Some(wire.expected_key);
                            captured = true;
                            crate::log(
                                "CALL: offer lane key captured at commit — basket egg secured",
                            );
                        }
                    }
                    // The offer's EXPRESS copy fires HERE, not at send: only the commit knows the lane key, and the express payload carries it as the callee's basket egg (signal.rs). ts = the row's own eagle stamp, so offer_osc and the stale-offer gate agree on both ends whichever copy lands first.
                    if captured {
                        if let Some(ci) = contact_idx {
                            self.send_express_signal(
                                ci,
                                &sig,
                                done.eagle_time,
                                Some(wire.expected_key),
                            );
                        }
                    }
                }
            }
            // Durable-then-transmit: snapshot now WITH the pending recorded; the transmit fires from the writer after the write lands.
            let snapshot = self
                .friendship_chains
                .iter()
                .find(|(id, _)| *id == done.friendship_id)
                .map(|(_, c)| c.clone());
            let dispatch = self.status_checker.as_ref().map(|c| c.message_dispatch());
            match (snapshot, dispatch) {
                (Some(snapshot), Some(dispatch)) => {
                    let req = crate::network::status::MessageRequest {
                        peer_addr: done.peer_addr,
                        alt_addr: done.alt_addr,
                        recipient_pubkey: done.recipient_pubkey,
                        conversation_token: done.conversation_token,
                        lane: wire.lane,
                        prev_msg_hp: wire.prev_msg_hp,
                        ciphertext: wire.ciphertext,
                        eagle_time: done.eagle_time,
                        relay_to: done.relay_to,
                    };
                    self.persist_chains_then(
                        snapshot,
                        vec![ChainsPostDurable::Message(dispatch, req)],
                    );
                    crate::logf!(
                        "CHAT: message ({} chars) committed — transmit rides the durable chains write",
                        done.text_len
                    );
                }
                (Some(snapshot), None) => {
                    // No checker (shutdown race): the advance still persists; the retransmit sweep re-serves the pending next session.
                    self.persist_chains_then(snapshot, Vec::new());
                }
                _ => {}
            }
            // The encrypt gate just opened — release the next held row (window permitting) on this commit edge. The just-committed row is pending now, so the sweep's idempotency gate skips it.
            if let Some(ci) = contact_idx {
                self.resend_held_messages(ci);
            }
        }
    }

    /// Run the deferred WIRE half of queued sends — one tick after their bubbles rendered. Failure handling matches the old inline path: no local chain but a sibling exists → fleet-forward (the sibling's merge drain transmits on the braid); no chain and no sibling → the send has nowhere to go, take the bubble back out.
    pub(super) fn drain_pending_chain_sends(&mut self) -> bool {
        if self.pending_chain_sends.is_empty() {
            return false;
        }
        // FRAME FENCE: only take entries queued on an EARLIER tick. The drain runs in the same tick pass that renders, so an entry queued by this pass's input handler would still hold the frame hostage thru the whole wire half -- exactly the void this deferral exists to kill (field-observed surviving the first version of it).
        let serial = self.tick_serial;
        let (sends, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending_chain_sends)
            .into_iter()
            .partition(|(_, _, _, _, _, q)| q.wrapping_add(1) < serial);
        self.pending_chain_sends = keep;
        if sends.is_empty() {
            return false;
        }
        for (ci, text, eagle_time, reference, bridge, _) in sends {
            let mut msg = ChatMessage::new_with_timestamp(text.clone(), true, eagle_time);
            msg.reference = reference;
            if !self.chain_transmit(ci, &text, eagle_time, reference, bridge.as_ref()) {
                let has_fleet = self.contacts.iter().any(|c| c.is_sibling);
                let is_friend = self.contacts.get(ci).map_or(false, |c| !c.is_sibling);
                if !(has_fleet && is_friend) {
                    // The bubble STAYS. Withdrawing it deleted what the user typed — and the commonest reason to land here is a re-key in flight (a wire flag-day, a peer that lost its chains), which resolves in seconds. The row is already persisted and marked undelivered, so it renders dim, survives a relaunch, and goes out the moment a chain exists — the held-message flush (resend_held_messages) sends it once a lane and a send slot are available.
                    self.persist_messages_async(ci);
                    crate::logf!(
                        "CHAT: no chain yet for this contact — message held undelivered, will send when the ceremony completes ({} queued)",
                        self.conv_of(ci)
                            .map(|v| v.messages.iter().filter(|m| m.is_outgoing && !m.delivered).count())
                            .unwrap_or(0)
                    );
                    continue;
                }
                crate::log("CHAT: no local chain — fleet-forwarded to the chain-owning sibling (delivered tick follows its ACK)");
            }
            // Live fleet propagation: our own outgoing message exists ONLY on this device until a sibling hears about it. (Same push carries the fleet-forward case.)
            self.push_rows_to_siblings(ci, std::slice::from_ref(&msg), None);
        }
        true
    }

    /// Resurrect the BridgeWire a re-serve must carry for a bridge output/control row — from the row's own stamped seq/exit (see the stamp in send_chain_message). Host/cwd stay None (the locus strip just doesn't refresh); a None here means the row is not a bridge frame or carries nothing to resurrect. Without this, every rebuilt final arrived wireless: the client painted the text but bridge_exit never stamped, the prompt gate held, and Stop hung the session (field thru 2026-08-30).
    pub(super) fn bridge_wire_for_row(
        &self,
        ci: usize,
        ts: i64,
    ) -> Option<crate::network::message_package::BridgeWire> {
        let conv = self.conv_of(ci)?;
        let m = conv.messages.iter().find(|m| m.is_outgoing && m.timestamp == ts)?;
        match m.reference {
            Some((crate::types::RefKind::BridgeOut, _)) => {
                Some(crate::network::message_package::BridgeWire {
                    host: None,
                    cwd: None,
                    seq: (m.bridge_seq > 0).then_some(m.bridge_seq),
                    exit: m.bridge_exit,
                    sig: None,
                    // Every v82+ host frame is a delta; resurrecting without the flag would make a re-served exit frame REPLACE the client's appended transcript with just the last chunk.
                    delta: true,
                })
            }
            _ => None,
        }
    }

    /// Send every outgoing row this contact still holds as undelivered — the rows `drain_pending_chain_sends` HELD because no chain existed yet (typically a re-key in flight). Original timestamps are preserved, so the row identity is unchanged and the friend dedups anything it already has; a row that still can't go out simply stays held for the next attempt.
    pub(super) fn resend_held_messages(&mut self, ci: usize) {
        let held: Vec<(String, i64, Option<(crate::types::RefKind, i64)>)> = match self.conv_of(ci)
        {
            Some(v) => v
                .messages
                .iter()
                // A reaction RETRACT is a legal empty-content row — its reference makes it sendable; a plain empty row stays filtered (the liveness-probe artifact class).
                .filter(|m| {
                    m.is_outgoing
                        && !m.delivered
                        && (!m.content.is_empty() || m.reference.is_some())
                })
                .map(|m| (m.content.clone(), m.timestamp, m.reference))
                .collect(),
            None => return,
        };
        if held.is_empty() {
            return;
        }
        let mut sent = 0usize;
        for (text, eagle_time, reference) in &held {
            let bw = self.bridge_wire_for_row(ci, *eagle_time);
            if self.chain_transmit(ci, text, *eagle_time, *reference, bw.as_ref()) {
                sent += 1;
            }
        }
        crate::logf!(
            "CHAT: chain ready — flushed {}/{} held message(s)",
            sent,
            held.len()
        );
    }

    /// The WIRE half of a chain send — weave selection, braid advance (prepare_send), chains persist, PT dispatch — with NO row bookkeeping: callers own the bubble. `send_chain_message` inserts its fresh bubble after; the fleet-forward drain transmits rows a sibling already merged (with their ORIGINAL timestamps, so the row identity — and therefore delivered-upgrades and digests — stays one fleet-wide). Returns false quietly when this device holds no usable chain (not Complete, no friendship chain, no party id, no address).
    pub(super) fn chain_transmit(
        &mut self,
        ci: usize,
        text: &str,
        eagle_time: i64,
        reference: Option<(crate::types::RefKind, i64)>,
        bridge: Option<&crate::network::message_package::BridgeWire>,
    ) -> bool {
        // Contact must be CLUTCH-Complete with a friendship chain — OR hold the sibling-replicated chains with a live lane root. Local Complete is only the ceremony OWNER's shape (§4.2 parks every other device at Pending forever), and gating on it made the owner the single writer: every other device fleet-forwarded thru it, which parks messages behind a dead battery an ocean away (Nick, 2026-08-13). Per-device lanes end that: `prepare_send` mints THIS device's own lane, the friend materializes it from the wire label (`ensure_lane`), and the lane-wise CRDT merge converges every copy — so holding the root is the whole capability.
        let (friendship_id, recipient_pubkey, addr_pair, _our_handle_hash, msg_relay_to) = {
            let Some(contact) = self.contacts.get(ci) else {
                return false;
            };
            let Some(fid) = contact.friendship_id else {
                crate::log("CHAT: cannot send — no friendship chain");
                return false;
            };
            let lane_capable = !contact.is_sibling
                && self
                    .friendship_chains
                    .iter()
                    .any(|(id, c)| *id == fid && c.lane_capable());
            if contact.clutch_state != crate::types::ClutchState::Complete && !lane_capable {
                crate::log("CHAT: cannot send — CLUTCH not complete");
                return false;
            }
            // Party id per contact: identity seed for friends, device-derived pid for fleet siblings — the chain index in prepare_send must match what from_clutch was keyed with.
            let Some(our_pid) = self.our_party_id(contact) else {
                return false;
            };
            // No direct path → also relay this message over the pipe.
            // CHAT joins the ACKs' rule: ALWAYS carry the relay copy. The direct-trust heuristic starved every shape of one-way reachability the field produced (a validated path to the wrong device, an AP that began isolating clients, a peer that left the LAN mid-session — messages gave up after 8 attempts while the always-relayed ACKs sailed thru, 2026-08-05). Receivers dedup by eagle_time, the well expires unclaimed copies, and a few hundred relayed bytes per message is nothing against a retransmit ladder burning minutes.
            let relay_to = contact.relay_device_list();
            // The wire recipient is a DEVICE key; a sendable contact always has one, but never fabricate for a keyless row.
            let Some(recipient_key) = contact.device_key() else {
                crate::log("CHAT: cannot send — no device key for contact");
                return false;
            };
            (
                fid,
                recipient_key,
                contact.race_addrs(),
                our_pid,
                relay_to,
            )
        };
        // IN-FLIGHT WINDOW: advance-on-send gives each message its own position, so pipelining is safe — but keep a bounded window so a burst can't outrun the receiver's gap buffer (and stays well under the count that tripped older receivers' fork detector). While the lane already holds the window's worth of un-ACKed sends, the row stays held and the ACK-advance flush sends the next as a slot frees.
        // CONTROL FRAMES BYPASS THE WINDOW. Call signals (offer/answer/decline/hangup), chain probes, and delete markers are rare, never bursty, and TIME-CRITICAL — pacing them behind bulk chat wedged a live call's answer the moment the lane hit its cap: "answer send failed" was every time preceded by "lane at the in-flight window", so a congested conversation made an incoming call literally unanswerable (decline worked only because it ignores the send result; field 2026-08-19, Emma↔Nick). The window is UI-level flow control for data, not a crypto invariant — a couple of extra control pendings stay far under the fork threshold and still ride advance-on-send + retransmit + relay like any frame.
        let is_control = crate::types::is_control_content(text);
        if !is_control
            && self
                .friendship_chains
                .iter()
                .find(|(id, _)| *id == friendship_id)
                .map_or(false, |(_, c)| {
                    c.pending_messages.len() >= crate::types::friendship::IN_FLIGHT_WINDOW
                })
        {
            crate::logf!(
                "CHAT: lane at the in-flight window ({}) — holding this message for an ACK slot",
                crate::types::friendship::IN_FLIGHT_WINDOW
            );
            return false;
        }

        // IDEMPOTENT PER MESSAGE: if this eagle_time is ALREADY pending, it was sent once and is in flight — the retransmit sweep resends its FROZEN ciphertext. Re-encrypting here would mint a fresh random pad (a different plaintext_hash) AND, under advance-on-send, ratchet the lane a SECOND time — double-advancing the position and forking it, so the peer's ACK (bound to the first hash) can never clear the pending and the message retransmits forever (field, 2026-08-08: Emma re-ACKing one message every ~2s, Nick never once logging "ACK verified"). resend_held_messages re-invokes this for every undelivered row, so the guard lives here, not only at that caller.
        if self
            .friendship_chains
            .iter()
            .find(|(id, _)| *id == friendship_id)
            .map_or(false, |(_, c)| {
                c.pending_messages
                    .iter()
                    .any(|m| m.eagle_time == eagle_time)
            })
        {
            crate::logf!("CHAT: message at eagle_time {} already in flight — leaving it to the retransmit sweep, not re-encrypting", eagle_time);
            return true;
        }

        // NO direct address is not NO send: the weave probe fired the moment a ceremony completed over the relay, hit this bail (the peer's addresses hadn't validated yet), and died silently — the probe never retransmits, so "testing the secure channel" sat forever on a chain that provably worked one direction (live pair, 2026-08-06). Same shape as the retransmit sweep: hand the sentinel so the UDP leg sends nowhere harmlessly and the relay copy carries it.
        let (peer_addr, alt_addr) = match addr_pair {
            Some(pair) => pair,
            None if !msg_relay_to.is_empty() => (crate::network::status::RELAY_ADDR, None),
            None => {
                crate::log("CHAT: cannot send — no known address for contact");
                return false;
            }
        };

        // The braid: choose up to TWO distinct prior PEER messages to weave into this chain step. Eligible = incoming messages (is_outgoing == false) in the last ≤256 of this conversation — any stored incoming row is one the receive path already ACKed, so the sender knows the peer holds it (both-held → identical strands → lockstep). The weave ingredient is the message's x-text (`content`), recoverable identically on both sides from the message DB. Each chosen message's eagle_time goes on the wire so the receiver resolves the SAME content. 0 eligible → weave nothing (anchor). 1 → single strand. ≥2 → two distinct (a true braid). Pick with gen_range (bounded, bias-free) — NEVER modulo. Strands are sorted by eagle_time so both peers frame derive_fresh_link identically regardless of pick order.
        // BRIDGE lanes are ANCHOR-ONLY: a sibling command/output frame weaves ZERO strands and requires none on receive. The braid's extra entropy is a friend-conversation property; a fleet-internal bridge is already fleet-key secured and still ratchets via the incorporated hp each step, so dropping the weave costs no real secrecy. The payoff is what Nick wants: with no strand dependency the terminal rows can be EPHEMERAL (wiped on open, never persisted) without ever producing a "braid strand miss" that holds a reply forever (field 2026-08-22). Anchor is an already-supported case (0 eligible → weave nothing) — this just forces it for siblings.
        let anchor_only = self.contacts.get(ci).map_or(false, |c| c.is_sibling);
        let (woven_strands, woven_times): (Vec<Vec<u8>>, Vec<i64>) = {
            let mut chosen: Vec<(i64, Vec<u8>)> = Vec::new();
            if let Some(conv) = self.conv_of(ci).filter(|_| !anchor_only) {
                let window: Vec<&crate::types::ChatMessage> = conv
                    .messages
                    .iter()
                    .rev()
                    // Probe rows are excluded from weave eligibility: they persist locally for re-ACK durability, but the PEER stores no outgoing row for its probe, so a woven probe ref would be unresolvable on their side — a guaranteed strand miss and chain fork.
                    .filter(|m| !m.is_outgoing && !crate::types::is_control_content(&m.content))
                    .take(256)
                    .collect();
                use rand::Rng;
                let mut rng = rand::thread_rng();
                if window.len() == 1 {
                    let m = window[0];
                    chosen.push((m.timestamp, m.content.as_bytes().to_vec()));
                } else if window.len() >= 2 {
                    let i = rng.gen_range(0..window.len());
                    let mut j = rng.gen_range(0..window.len() - 1);
                    if j >= i {
                        j += 1; // map [0, len-1) → [0, len)\{i} so j is distinct from i, uniformly
                    }
                    for &idx in &[i, j] {
                        let m = window[idx];
                        chosen.push((m.timestamp, m.content.as_bytes().to_vec()));
                    }
                }
            }
            chosen.sort_by_key(|(t, _)| *t);
            let times = chosen.iter().map(|(t, _)| *t).collect();
            let strands = chosen.into_iter().map(|(_, c)| c).collect();
            (strands, times)
        };

        // Build the MESSAGE PACKAGE the receiver parses: a complete framed VSF document (AGENT.md "COMPLETE VSF FILES ONLY") — body, incorporated hp, woven strand times, the typed reference, and a random pad, every one a named schema field. Describing something new about a message is adding a field, never an encoding trick.
        // ENCRYPT-IN-FLIGHT GATE: one braid encrypt per friendship at a time — a second dispatch would mint a second frame at the SAME lane position, and the commit CAS would void it. Returning false keeps the row held-undelivered (the bubble stays); the commit edge re-fires it thru resend_held_messages.
        if self.send_encrypt_busy.contains(friendship_id.as_bytes()) {
            crate::log("CHAT: braid encrypt already in flight for this friendship — row held for the commit edge");
            return false;
        }

        // Build the wire payload + snapshot the chains, then hand the memory-hard braid encrypt to a worker — the scratch ran inline here, on the Enter keypress. The commit (CAS + advance + pending + durable transmit) runs in drain_braid_tx when the worker posts back.
        let (snapshot, payload, salt_text, conversation_token) = {
            let Some((_, chains)) = self
                .friendship_chains
                .iter_mut()
                .find(|(id, _)| *id == friendship_id)
            else {
                crate::log("CHAT: friendship chains missing for open contact");
                return false;
            };
            // Mint-our-lane stays inline: once per era, and the encrypt snapshot must carry the minted label.
            if chains.mint_our_lane().is_none() {
                crate::log("CHAT: prepare_send failed (no lane root — pre-lanes chains)");
                return false;
            }
            let incorporated_hp = chains
                .last_incorporated_hp()
                .map(|h| *h)
                .unwrap_or([0u8; 32]);
            // Short random pad (median ~53B) for traffic-analysis size jitter; the schema parses by NAME, the stronger form of what the old field-order shuffle enforced.
            let pad_len = rand::random::<u8>()
                .min(rand::random::<u8>())
                .min(rand::random::<u8>()) as usize;
            let pad: Vec<u8> = (0..pad_len).map(|_| rand::random()).collect();
            let payload = match crate::network::message_package::build_message_package(
                text,
                &incorporated_hp,
                &woven_times,
                reference.map(|(k, t)| (k as u8, t)),
                bridge,
                &pad,
            ) {
                Ok(p) => p,
                Err(e) => {
                    crate::logf!("CHAT: message package build failed: {}", e);
                    return false;
                }
            };
            // Chain ingredient = the bare x-text only (the hp/hR pad are siblings of x in the field, not part of it, and are never chain-key material). The full `payload` is what's encrypted onto the wire; `text` is what salts/advances the chain.
            let salt_text = text.to_string().into_bytes();
            let token = chains.conversation_token;
            (chains.clone(), payload, salt_text, token)
        };
        self.send_encrypt_busy.insert(*friendship_id.as_bytes());
        let tx = self.braid_tx_tx.clone();
        let wake = self.event_proxy.clone();
        let text_len = text.len();
        queue_job(&self.braid_job_tx, move || {
            let result = snapshot.prepare_send_encrypt(&payload, eagle_time).map(
                |(ciphertext, prev_msg_hp, msg_hp, plaintext_hash, lane, expected_key)| {
                    BraidTxWire {
                        ciphertext,
                        prev_msg_hp,
                        msg_hp,
                        plaintext_hash,
                        lane,
                        expected_key,
                    }
                },
            );
            let _ = tx.send(BraidTxEncrypted {
                friendship_id,
                conversation_token,
                eagle_time,
                salt_text,
                woven_strands,
                peer_addr,
                alt_addr,
                recipient_pubkey,
                relay_to: msg_relay_to,
                text_len,
                result,
            });
            if let Some(w) = wake.as_ref() {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });

        true
    }

    /// Just after a contact's CLUTCH reaches `Complete`, fire the one hidden chain-weave probe: a normal chat message with the reserved [`CHAIN_PROBE_MARKER`] content, sent once (guarded by `probe_sent`) with its UI bubble suppressed. When it lands the peer advances+ACKs the chain like any message, which is what proves the ratchet works end-to-end without the user seeing a decoy message. No-op if the contact isn't Complete, has no friendship chain yet, or already probed. Skips self-contacts (no peer to answer). Consolidates the transition-site logic so every `= ClutchState::Complete` path only needs one call.
    pub(super) fn maybe_send_chain_probe(&mut self, contact_idx: usize) {
        let should_send = match self.contacts.get(contact_idx) {
            Some(c) => {
                c.clutch_state == crate::types::ClutchState::Complete
                    && c.friendship_id.is_some()
                    && !c.probe_sent
                    // A LANE AT ITS WINDOW needs no probe: an un-ACKed message already in flight proves what the probe would, and it advances the chain. Without this the send window and the presence re-probe sweep spun against each other — probe attempted, held, never latched, re-attempted every frame (196 attempts in 3.4s on a phone, 2026-08-07). The probe fires later, from the same sweep, once a slot frees.
                    && c.friendship_id.is_none_or(|fid| {
                        self.friendship_chains
                            .iter()
                            .find(|(id, _)| *id == fid)
                            .is_none_or(|(_, ch)| {
                                ch.pending_messages.len()
                                    < crate::types::friendship::IN_FLIGHT_WINDOW
                            })
                    })
                    // A weave probe proves a chain reaches someone; with no remote participants there is no chain and nobody to answer it.
                    && self.our_party_id(c).is_some_and(|us| c.remote_count(&us) > 0)
            }
            None => false,
        };
        if !should_send {
            return;
        }
        crate::log("CHAIN-PROBE: sending hidden chain-weave probe");
        // Latch `probe_sent` only on an actual dispatch — if the contact had no address yet the send is a no-op and we retry on the next Complete transition / re-arm cycle rather than stalling.
        if self.send_chain_message(contact_idx, crate::types::CHAIN_PROBE_MARKER, true, None, None) {
            if let Some(c) = self.contacts.get_mut(contact_idx) {
                c.probe_sent = true;
            }
        }
    }

    /// Mark the chain end-to-end proven (`chain_woven = true`) once BOTH directions are validated: we've seen the peer's probe (their TX / our RX proven, `their_probe_seen`) AND our own chain has advanced via an ACK at least once (`chain_advanced_by_ack`, our TX / their RX proven). On seal, kill the ceremony proof rebroadcast (`clutch_proof_resends_left = 0`) so the completed CLUTCH stops re-announcing, flip the status line from "weaving the chain" to "secured", and persist. Idempotent — safe to call from either the probe-receive path or the ACK path. The chain math itself is never touched here.
    pub(super) fn seal_chain_if_ready(&mut self, contact_idx: usize) {
        let Some(c) = self.contacts.get_mut(contact_idx) else {
            return;
        };
        if c.chain_woven {
            return;
        }
        if !(c.their_probe_seen && c.chain_advanced_by_ack) {
            // Not sealed yet. If OUR half is the missing one, our probe may simply have been lost — one best-effort frame, and losing it deadlocks the pair forever (each side ends up holding a different half). This runs on the EDGE of a proof arriving, never on a timer, and is bounded so an unreachable peer doesn't re-probe endlessly.
            if c.their_probe_seen
                && !c.chain_advanced_by_ack
                && c.probe_sent
                && c.probe_resends_left > 0
            {
                c.probe_resends_left -= 1;
                c.probe_sent = false;
                let left = c.probe_resends_left;
                crate::logf!(
                    "CHAIN-PROBE: their probe landed but ours was never acked — re-arming our probe ({} left)",
                    left
                );
                self.maybe_send_chain_probe(contact_idx);
            }
            return;
        }
        c.chain_woven = true;
        c.clutch_completed_at = Some(std::time::Instant::now()); // refresh the re-key cooldown thru the weave (armed at completion; this extends it)
        c.clutch_proof_resends_left = 0;
        // §4.2 owner backfill: a weave sealed HERE makes this device the friendship's chain holder — stamp the claim if none exists (pre-§4.2 ceremonies never claimed), so siblings' rosters learn a REAL owner device and their status lines can name where to send from instead of guessing.
        if !c.is_sibling && c.ceremony_owner.is_none() {
            if let Some(our_device) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()) {
                let c = &mut self.contacts[contact_idx];
                c.ceremony_owner = Some(our_device);
                c.roster_updated = vsf::eagle_time_oscillations();
            }
        }
        let Some(c) = self.contacts.get_mut(contact_idx) else {
            return;
        };
        crate::log(
            "CHAIN-PROBE: chain woven — end-to-end verified, ceremony rebroadcast cancelled",
        );
        // A fresh weave re-opens the blind conversation with this friend: re-probe for a deposit (reset side) and allow a fresh put (their reset wiped nothing of ours, but a re-key on OUR side after []n starts from scratch).
        c.blind_probe_missed = false;
        c.blind_in_flight = None;
        // Kick off friend-history recovery on the woven-chain EDGE (this fn fires exactly once per seal; vault loads latch chain_woven without passing here, so restarts resume via the persisted cursor instead of re-kicking). Always request the head page — if we already hold the history, merging dedups and the early-stop rule completes after one page. Siblings are excluded: friend-history recovery resolves "the other participant ≠ our seed", which is ambiguous on a sibling chain — fleet history sync is its own later phase.
        let is_sibling = c.is_sibling;
        if !is_sibling {
            if let Some(conv) = self.conv_mut_of(contact_idx) {
                let was_complete_before = conv
                    .history_recovery
                    .as_ref()
                    .map(|r| r.complete)
                    .unwrap_or(false);
                conv.history_recovery = Some(crate::types::HistoryRecovery {
                    oldest_recovered_osc: i64::MAX,
                    complete: false,
                    in_flight: None,
                    next_request_osc: 0,
                    urgent: true, // head page jumps the trickle interval — conversation usable ASAP
                    was_complete_before,
                    decrypt_fail_streak: 0,
                    expire_streak: 0,
                    parked_key_fp: None,
                });
                crate::log("HISTORY: recovery kicked off (head page next tick)");
            }
        }
        if let Some(storage) = self.storage.as_ref() {
            if let Err(e) =
                crate::storage::contacts::save_contact(&self.contacts[contact_idx], storage)
            {
                crate::logf!("CHAIN-PROBE: failed to persist woven contact: {}", e);
            }
        }
        // §4.2: the seal is the moment `woven` becomes true in OUR roster entry — push it so parked siblings flip from "weaving on <device>…" to "secured on <device>". Bump the LWW clock so the woven entry actually wins the merge.
        if !is_sibling {
            if let Some(c) = self.contacts.get_mut(contact_idx) {
                c.roster_updated = vsf::eagle_time_oscillations();
            }
            self.spawn_roster_push();
        }
        // Last, past every contact borrow: flush anything the user typed while the chain was missing (a re-key in flight). Those rows were HELD, not withdrawn, so they go out now with their ORIGINAL timestamps — the row identity stays one fleet-wide and the friend dedups a resend.
        self.resend_held_messages(contact_idx);
    }
}
