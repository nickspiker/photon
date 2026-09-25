//! Per-conversation compose DRAFTS (Nick 2026-09-25): the compose bar belongs to the conversation it was typed in.
//! Leaving a conversation stashes what was typed (text, tagged links, an armed reply/edit/reaction) and clears the bar; entering one puts its own draft back.
//! Durable on this device only (storage/drafts.rs), written off the UI thread on the leave edge, on blur and quit, the moment a draft empties (a send), and at most 60 s after an edit that nothing else wrote — the one deadline, Nick's grant in the same message.

use super::*;
use crate::storage::drafts::{Draft, DraftLink};

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
        }
    }

    /// Fold the open conversation's compose bar into the draft list; returns whether the stored draft changed.
    fn capture_draft(&mut self) -> bool {
        let Some(conv) = self.active_conversation else {
            return false;
        };
        let d = self.compose_draft();
        let pos = self.drafts.iter().position(|(c, _)| *c == conv);
        match (pos, d.is_empty()) {
            (Some(i), true) => {
                self.drafts.remove(i);
                true
            }
            (Some(i), false) if self.drafts[i].1 != d => {
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
    pub(super) fn draft_deadline(&self) -> Option<Instant> {
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
        }
        let mut changed = false;
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
