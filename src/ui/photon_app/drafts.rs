//! Per-conversation compose DRAFTS (Nick 2026-09-25): the compose bar belongs to the conversation it was typed in.
//! Leaving a conversation stashes what was typed (text, tagged links, an armed reply/edit/reaction) and clears the bar; entering one puts its own draft back.
//! Durable (storage/drafts.rs), written off the UI thread on the leave edge, on blur and quit, the moment a draft empties (a send), and at most 60 s after an edit that nothing else wrote — the one deadline, Nick's grant in the same message.

use super::*;
use crate::storage::drafts::{Draft, DraftLink};

/// The fleet-state key prefix of a conversation's draft (`draft.<conversation id hex>`).
const DRAFT_KEY_PREFIX: &str = "draft.";

/// How long an edited draft may sit unwritten (Nick 2026-09-25: "let it sit for more than 60s it writes to disk").
const DRAFT_WRITE_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

/// What the compose bar last looked like: (edit counter, tagged-link count, reply, edit, react). A change is the edge that marks the open draft dirty.
pub(super) type ComposeSeen = (u64, usize, Option<i64>, Option<i64>, Option<i64>);

impl PhotonApp {
    fn compose_seen(&self) -> Option<ComposeSeen> {
        let tb = self.message_textbox.as_ref()?;
        let links = tb.spans().iter().filter(|s| s.dest.is_some()).count();
        Some((tb.edit_seq(), links, self.compose_reply_to, self.compose_edit_of, self.compose_react_to))
    }

    /// The compose bar as a draft: the text, its TAGGED links (bare URLs are re-detected from the text), and the armed mode.
    fn compose_draft(&self) -> Draft {
        let Some(tb) = self.message_textbox.as_ref() else {
            return Draft::default();
        };
        Draft {
            text: tb.chars.iter().collect(),
            links: tb
                .spans()
                .iter()
                .filter_map(|s| s.dest.as_ref().map(|d| DraftLink { start: s.start, end: s.end, dest: d.clone() }))
                .collect(),
            reply_to: self.compose_reply_to,
            edit_of: self.compose_edit_of,
            react_to: self.compose_react_to,
            updated: 0,
        }
    }

    /// Fold the open conversation's compose bar into the draft list; returns whether the stored draft changed.
    fn capture_draft(&mut self) -> bool {
        let Some(conv) = self.active_conversation else {
            return false;
        };
        let mut d = self.compose_draft();
        let pos = self.drafts.iter().position(|(c, _)| *c == conv);
        // The stamp is the moment the content changed (captured on the edit edge itself): the clock the fleet's newest-wins runs on.
        d.updated = vsf::eagle_time_oscillations();
        match (pos, d.is_empty()) {
            (Some(i), true) => {
                self.drafts.remove(i);
                true
            }
            (Some(i), false) if !self.drafts[i].1.same_content(&d) => {
                self.drafts[i].1 = d;
                true
            }
            (None, false) => {
                self.drafts.push((conv, d));
                true
            }
            _ => false,
        }
    }

    /// Write every draft, off the UI thread (the one job worker keeps writes in order, so a later snapshot always lands last).
    /// Before the vault's copy has loaded, a write would clobber drafts this session has not seen yet — the dirty mark stays and the load's arrival writes the merge.
    pub(super) fn persist_drafts(&mut self) {
        if !self.drafts_loaded {
            return;
        }
        let Some(storage) = self.storage.as_ref().cloned() else {
            return;
        };
        self.draft_dirty_since = None;
        let snapshot = self.drafts.clone();
        let pending = self.durable_pending.clone();
        *self.durable_pending.0.lock().unwrap() += 1;
        queue_job(&self.seal_job_tx, move || {
            if let Err(e) = crate::storage::drafts::save_drafts(&snapshot, &storage) {
                crate::logf!("STORAGE: drafts write failed ({} draft(s) stay in RAM, the next edge retries): {}", snapshot.len(), e);
            }
            // Quit-drain accounting — the same law as the message writers: a typed draft must not die with a quitting process.
            let (n, cv) = &*pending;
            *n.lock().unwrap() -= 1; // counted in above, before the queue
            cv.notify_all();
        });
        self.sync_drafts_to_fleet();
    }

    /// The fleet-state key of a conversation's draft.
    fn draft_key(conv: &crate::types::ConversationId) -> String {
        format!("{}{}", DRAFT_KEY_PREFIX, hex::encode(conv.as_bytes()))
    }

    /// Conversations that exist only between two of our own devices (sibling chats): their ids mean nothing elsewhere, so their drafts stay on this device.
    fn device_pair_conversations(&self) -> crate::linear_map::SortedSet<[u8; 32]> {
        self.contacts.iter().filter(|c| c.is_sibling).filter_map(|c| c.friendship_id.map(|f| *f.as_bytes())).collect()
    }

    /// THE FLEET'S DRAFTS (Nick 2026-10-15: "upon draft save I think that should sync fleetwide… timestamp it so if it's newer than text already in the box, REPLACE!"): every save edge also puts each draft into the fleet state as `draft.<conversation>` — sealed with the fleet key, stamped with the draft's own last-edit time, so the newest edit wins on every device — and a draft that went away (a send, a clear) becomes a tombstone. Only what moved is written; the slot is pushed off the UI thread.
    fn sync_drafts_to_fleet(&mut self) {
        if !self.drafts_loaded || !self.ensure_fleet_settings() {
            return;
        }
        let local_only = self.device_pair_conversations();
        let now = vsf::eagle_time_oscillations();
        let drafts = self.drafts.clone();
        let fs = self.fleet_settings.as_mut().unwrap();
        let mut changed = 0usize;
        for (conv, d) in &drafts {
            if local_only.contains(conv.as_bytes()) {
                continue;
            }
            let key = Self::draft_key(conv);
            // The fleet already holds this edit or a newer one (or a newer tombstone — a send elsewhere after this edit): nothing to say.
            if fs.global.iter().any(|e| e.key == key && e.updated >= d.updated) {
                continue;
            }
            let Ok(bytes) = crate::storage::drafts::draft_to_bytes(*conv, d) else { continue };
            if fs.set(&key, vsf::VsfType::hR(bytes), d.updated) {
                changed += 1;
            }
        }
        // A draft the fleet still holds but this device no longer does was sent or cleared HERE: tombstone it so no device brings it back.
        let gone: Vec<String> = fs
            .global
            .iter()
            .filter(|e| e.key.starts_with(DRAFT_KEY_PREFIX) && !e.tombstone && e.updated < now)
            .filter(|e| !drafts.iter().any(|(c, _)| Self::draft_key(c) == e.key))
            .map(|e| e.key.clone())
            .collect();
        for key in gone {
            if fs.tombstone_key(&key, now) {
                changed += 1;
            }
        }
        if changed == 0 {
            return;
        }
        crate::logf!("DRAFTS: {} draft change(s) to the fleet", changed);
        if let (Some(fs), Some(storage)) = (self.fleet_settings.as_ref(), self.storage.as_ref().cloned()) {
            let snapshot = fs.clone();
            queue_job(&self.seal_job_tx, move || {
                if let Err(e) = crate::storage::fleet_settings::save_fleet_settings(&snapshot, &storage) {
                    crate::logf!("SETTINGS: persist after draft sync failed: {}", e);
                }
            });
        }
        self.spawn_settings_push();
    }

    /// Drafts that arrived in the fleet state (a sibling's save edge): each one NEWER than this device's copy replaces it — and when its conversation is open, replaces what is in the compose bar too (Nick: "if it's newer than text already in the box, REPLACE!"). A newer tombstone removes the draft (it was sent elsewhere). The bar's own last edit is the stored stamp, captured on every edit edge, so typing here after the remote edit keeps what was typed here.
    pub(super) fn merge_remote_drafts(&mut self) {
        if !self.drafts_loaded {
            return; // the load's arrival merges
        }
        let Some(fs) = self.fleet_settings.as_ref() else { return };
        let local_only = self.device_pair_conversations();
        let mut incoming: Vec<(crate::types::ConversationId, i64, Option<crate::storage::drafts::Draft>)> = Vec::new();
        for e in fs.global.iter().filter(|e| e.key.starts_with(DRAFT_KEY_PREFIX)) {
            let Some(conv) = hex::decode(&e.key[DRAFT_KEY_PREFIX.len()..]).ok().and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok()).map(crate::types::ConversationId::from_bytes) else {
                continue;
            };
            if local_only.contains(conv.as_bytes()) {
                continue;
            }
            if e.tombstone {
                incoming.push((conv, e.updated, None));
            } else if let Some((_, d)) = crate::storage::fleet_settings::as_bytes(&e.value).and_then(|b| crate::storage::drafts::draft_from_bytes(&b)) {
                incoming.push((conv, e.updated.max(d.updated), Some(d)));
            }
        }
        let mut changed = false;
        for (conv, updated, d) in incoming {
            let pos = self.drafts.iter().position(|(c, _)| *c == conv);
            if pos.is_some_and(|i| self.drafts[i].1.updated >= updated) {
                continue; // ours is as new or newer
            }
            match (d, pos) {
                (None, Some(i)) => {
                    self.drafts.remove(i);
                }
                (None, None) => continue,
                (Some(mut d), Some(i)) => {
                    d.updated = updated;
                    self.drafts[i].1 = d;
                }
                (Some(mut d), None) => {
                    d.updated = updated;
                    self.drafts.push((conv, d));
                }
            }
            changed = true;
            if self.active_conversation == Some(conv) {
                self.draft_replace_pending = true;
            }
        }
        if changed {
            crate::log("DRAFTS: newer draft(s) from another device adopted");
            self.draft_dirty_since.get_or_insert_with(Instant::now);
            self.persist_drafts();
        }
    }

    /// Capture the open draft and write it now if anything is unwritten — the blur, quit and leave edges.
    pub(super) fn flush_draft(&mut self) {
        if self.capture_draft() {
            self.draft_dirty_since.get_or_insert_with(Instant::now);
        }
        if self.draft_dirty_since.is_some() {
            self.persist_drafts();
        }
    }

    /// THE one door to changing the open conversation: the compose bar travels with the conversation it was typed in, never into the next one.
    pub(super) fn set_active_conversation(&mut self, next: Option<crate::types::ConversationId>) {
        if next == self.active_conversation {
            return;
        }
        self.flush_draft();
        if let Some(tb) = self.message_textbox.as_mut() {
            tb.clear();
        }
        // An armed reply/edit/react targets a row of the conversation it was armed IN — the next conversation's own draft re-arms its mode.
        self.compose_reply_to = None;
        self.compose_edit_of = None;
        self.compose_react_to = None;
        self.active_conversation = next;
        // Putting the draft back needs the text renderer, which the tick holds.
        self.draft_restore_pending = next.is_some();
        self.draft_seen = None;
    }

    /// Forget every draft without writing — the device wipe (a wipe must never re-persist what it just deleted).
    pub(super) fn forget_drafts(&mut self) {
        self.drafts.clear();
        self.drafts_loaded = false;
        self.drafts_loading = false;
        self.draft_dirty_since = None;
        self.draft_restore_pending = false;
        self.draft_seen = None;
        self.drafts_rx = None;
    }

    /// The draft write deadline, for the wake schedule.
    /// None while persist_drafts would refuse (vault copy not loaded yet, or no vault): a deadline it cannot clear would sit in the past and spin the loop, and the load's arrival is the edge that writes instead.
    pub(super) fn draft_deadline(&self) -> Option<Instant> {
        if !self.drafts_loaded || self.storage.is_none() {
            return None;
        }
        self.draft_dirty_since.map(|t| t + DRAFT_WRITE_AFTER)
    }

    /// Per tick: load the vault's drafts once, put the open conversation's draft back, notice edits, and write on the deadline. Returns whether the compose bar changed.
    pub(super) fn drafts_tick(&mut self, now: Instant, text: &mut fluor::text::TextRenderer) -> bool {
        if !self.drafts_loaded && !self.drafts_loading {
            if let Some(storage) = self.storage.as_ref().cloned() {
                self.drafts_loading = true;
                let (tx, rx) = std::sync::mpsc::channel();
                self.drafts_rx = Some(rx);
                let wake = self.event_proxy.clone();
                queue_job(&self.seal_job_tx, move || {
                    let loaded = crate::storage::drafts::load_drafts(&storage);
                    let _ = tx.send(loaded);
                    if let Some(w) = wake.as_ref() {
                        let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                    }
                });
            }
        }
        if let Some(loaded) = self.drafts_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.drafts_rx = None;
            self.drafts_loading = false;
            self.drafts_loaded = true;
            match loaded {
                Ok(stored) => {
                    // Anything typed this session before the load landed is newer than the vault's copy of that conversation.
                    for (c, d) in stored {
                        if !self.drafts.iter().any(|(k, _)| *k == c) {
                            self.drafts.push((c, d));
                        }
                    }
                }
                Err(e) => crate::logf!("STORAGE: drafts unreadable — starting empty, the next write replaces them: {}", e),
            }
            if self.draft_dirty_since.is_some() {
                self.persist_drafts();
            }
            // Drafts the fleet saved while this device's copy was loading.
            self.merge_remote_drafts();
        }
        let mut changed = false;
        // A newer draft from another device for the OPEN conversation: replace the bar outright (Nick: "if it's newer than text already in the box, REPLACE!"). The replacement is not an edit here — the bar's seen-state is re-read so it is not captured and stamped as a fresh one (no ping-pong between devices).
        if self.draft_replace_pending && !self.draft_restore_pending && self.message_textbox.is_some() {
            self.draft_replace_pending = false;
            let draft = self.active_conversation.and_then(|c| self.drafts.iter().find(|(k, _)| *k == c).map(|(_, d)| d.clone()));
            if let Some(tb) = self.message_textbox.as_mut() {
                tb.clear();
                match &draft {
                    Some(d) => {
                        tb.set_text(&d.text, text);
                        for l in &d.links {
                            tb.tag_link(l.start, l.end, l.dest.clone(), *theme::LINK_PURPLE);
                        }
                    }
                    None => {}
                }
            }
            self.compose_reply_to = draft.as_ref().and_then(|d| d.reply_to);
            self.compose_edit_of = draft.as_ref().and_then(|d| d.edit_of);
            self.compose_react_to = draft.as_ref().and_then(|d| d.react_to);
            self.draft_seen = self.compose_seen();
            // The phone's keyboard holds its own copy of the field: restart it so it does not re-materialise the replaced text.
            self.pending_input_reset = true;
            crate::log("DRAFTS: the compose bar took a newer draft from another device");
            changed = true;
        }
        if self.draft_restore_pending && self.drafts_loaded && self.message_textbox.is_some() {
            self.draft_restore_pending = false;
            let draft = self.active_conversation.and_then(|c| self.drafts.iter().find(|(k, _)| *k == c).map(|(_, d)| d.clone()));
            if let (Some(d), Some(tb)) = (draft, self.message_textbox.as_mut()) {
                // Only into an untouched bar: a human who started typing before the draft could load keeps what they typed.
                if tb.chars.is_empty() {
                    tb.set_text(&d.text, text);
                    for l in &d.links {
                        tb.tag_link(l.start, l.end, l.dest.clone(), *theme::LINK_PURPLE);
                    }
                    self.compose_reply_to = d.reply_to;
                    self.compose_edit_of = d.edit_of;
                    self.compose_react_to = d.react_to;
                    changed = true;
                }
            }
            self.draft_seen = self.compose_seen();
        } else if !self.draft_restore_pending {
            let seen = self.compose_seen();
            if seen != self.draft_seen {
                self.draft_seen = seen;
                if self.capture_draft() {
                    self.draft_dirty_since.get_or_insert(now);
                    // A draft that just emptied is a SEND (or a clear): write now, or a crash would bring back text that already went out.
                    let emptied = self.active_conversation.is_some_and(|c| !self.drafts.iter().any(|(k, _)| *k == c));
                    if emptied {
                        self.persist_drafts();
                    }
                }
            }
        }
        if self.draft_deadline().is_some_and(|t| now >= t) {
            self.persist_drafts();
        }
        changed
    }
}
