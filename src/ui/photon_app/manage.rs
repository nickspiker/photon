//! Contact MANAGE (Nick 2026-09-27): removal that sticks, and clearing a conversation's history.
//!
//! Relationship, history and reach are separate choices (docs/lifecycle.md). This module holds the two built so far:
//! - BOOT that sticks: the booted ledger (storage::booted) is re-emitted as a roster tombstone on every push and consulted by every path that can mint a contact, so a failed push, a re-seal, the cloud backup or a sibling's stale stamp can no longer bring a booted contact back.
//! - CLEAR HISTORY (all, or waves only), across our own fleet: every matching row is tombstoned — the same true-wins mark a single delete uses, so no sibling push or friend backfill refills it — and each recording or attachment, a distinct vault object, is deleted.
//!   The friend is not told: their copy is theirs (authorship propagates, experience doesn't).
//!   Row text stays at rest, encrypted and hidden, marked for discard: until we own the flash, discard is the honest verb (feedback: discard, not shred).

use super::*;
use crate::storage::booted::Booted;

/// Rows per sibling push while clearing: one push is one history page, and a long conversation must not become one page of thousands of rows.
const CLEAR_PUSH_BATCH: usize = 64;

impl PhotonApp {
    /// Write the booted ledger off the UI thread (the render thread never touches the vault).
    pub(super) fn persist_booted(&self) {
        let Some(storage) = self.storage.as_ref().cloned() else {
            return;
        };
        let snapshot = self.booted.clone();
        queue_job(&self.seal_job_tx, move || {
            if let Err(e) = crate::storage::booted::save_booted(&snapshot, &storage) {
                crate::logf!("BOOT: booted ledger write failed ({} entr(ies) stay in RAM, the next boot or merge retries): {}", snapshot.len(), e);
            }
        });
    }

    /// Record a boot (ours or a sibling's tombstone) so it sticks on this device and rides every roster push from here on.
    pub(super) fn note_booted(&mut self, handle_proof: [u8; 32], party_id: [u8; 32], at: i64) {
        if crate::storage::booted::note_boot(&mut self.booted, Booted { handle_proof, party_id, at }) {
            self.persist_booted();
        }
    }

    /// A deliberate re-add clears the boot: the contact is wanted again.
    pub(super) fn forget_booted(&mut self, handle_proof: &[u8; 32]) {
        let before = self.booted.len();
        self.booted.retain(|b| b.handle_proof != *handle_proof);
        if self.booted.len() != before {
            crate::logf!("BOOT: {} re-added — the boot is lifted", crate::fp(handle_proof));
            self.persist_booted();
        }
    }

    /// Does a recorded boot refuse a live roster entry stamped `updated`? Only a strictly newer entry (a deliberate re-add) gets thru.
    pub(super) fn booted_refuses(&self, handle_proof: &[u8; 32], updated: i64) -> bool {
        crate::storage::booted::boot_outranks(&self.booted, handle_proof, updated)
    }

    /// Is this party id one we booted? Chain adoption and the cloud merge ask before minting anything for it.
    pub(super) fn is_booted_party(&self, party_id: &[u8; 32]) -> bool {
        self.booted.iter().any(|b| b.party_id == *party_id)
    }

    /// Clear the ACTIVE contact's conversation across our fleet: every row (or only the waves and their recordings), marked deleted and pushed to every sibling; recordings and attachments deleted from the vault.
    pub(super) fn clear_active_history(&mut self, waves_only: bool) {
        let Some(ci) = self.active_contact() else {
            return;
        };
        if self.contacts[ci].is_sibling {
            return; // a sibling's conversation is the bridge terminal — it keeps no history to clear
        }
        let mut cleared: Vec<ChatMessage> = Vec::new();
        let mut discard_blobs: Vec<[u8; 32]> = Vec::new();
        if let Some(conv) = self.conv_mut_of(ci) {
            for m in conv.messages.iter_mut() {
                // Control rows are machinery, never shown and never a thing the human cleared; a row already deleted needs nothing.
                if m.deleted || m.is_control() {
                    continue;
                }
                let is_wave = m.wave.is_some()
                    || matches!(m.reference, Some((crate::types::RefKind::Wave, _)))
                    || m.file.as_ref().is_some_and(|f| matches!(f.role, crate::types::AttachRole::WaveAudio | crate::types::AttachRole::WaveEnv));
                if waves_only && !is_wave {
                    continue;
                }
                m.deleted = true;
                if let Some((hash, _, _)) = m.file_parts() {
                    discard_blobs.push(hash);
                }
                cleared.push(m.clone());
            }
            if !cleared.is_empty() {
                conv.invalidate_digest(); // tombstones drop rows from the syncable set
            }
        }
        crate::logf!(
            "MANAGE: cleared {} row(s) ({}) with {} across our fleet — {} recording/attachment blob(s) discarded; the friend's copy is theirs",
            cleared.len(),
            if waves_only { "waves" } else { "all history" },
            crate::fp(&self.contacts[ci].handle_proof),
            discard_blobs.len()
        );
        if cleared.is_empty() {
            return;
        }
        self.persist_messages_async(ci);
        for batch in cleared.chunks(CLEAR_PUSH_BATCH) {
            self.push_rows_to_siblings(ci, batch, None);
        }
        // Each blob is a distinct vault object: delete it outright, off the UI thread (a kept recording is chunked, one vault commit per chunk). Idempotent, and every sibling applying the tombstones deletes its own copies.
        if !discard_blobs.is_empty() {
            queue_job(&self.seal_job_tx, move || {
                for h in discard_blobs {
                    crate::storage::blob_delete(&h);
                }
            });
        }
        self.selected_msg = None;
        self.msg_wrap = None; // the row set changed
        self.scene_dirty = true;
    }
}

impl PhotonApp {
    /// The Manage page's clear pills on rows 5 and 6 (slots 2 and 3 of the contact panel's pill ids) and their note on row 7.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_clear_pills(
        armed_state: Option<bool>,
        pill_base: HitId,
        canvas: &mut Canvas,
        text: &mut fluor::text::TextRenderer,
        hit_map: &mut [HitId],
        buf_w: usize,
        buf_h: usize,
        rows: &[fluor::region::Region],
        span: Coord,
        pressed_hit: HitId,
    ) {
        for (row, slot, all) in [(rows[5], 2, false), (rows[6], 3, true)] {
            let pill = fluor::region::Region::new(row.x + row.w * 0.1, row.y, row.w * 0.5, row.h * 0.95);
            let armed = armed_state == Some(all);
            let label = if all { tr(Msg::ClearHistoryPill { armed }) } else { tr(Msg::ClearWavesPill { armed }) };
            draw_stub_pill(canvas, text, hit_map, buf_w, buf_h, pill, &label, pill_base + slot, pressed_hit);
        }
        settings_line(canvas, text, rows[7], &tr(Msg::ClearHistoryNote), span, *theme::LABEL_COLOUR, 400);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn clear_batches_stay_page_sized() {
        assert!(super::CLEAR_PUSH_BATCH > 0 && super::CLEAR_PUSH_BATCH <= 256);
    }
}
