//! A conversation: a set of participants and the messages between them.
//!
//! This is the object `Contact` was doing double duty as. A contact is a PEER — someone you ran a key exchange with, who has an address, a presence, an avatar. A conversation is a PLACE — a participant set and the rows in it. Conflating them is why notes-to-self needed ~60 special-cases across the tree: a conversation with no peer had to fake fourteen peer fields, and every subsystem that assumed "there is someone on the other end" patched around it locally.
//!
//! Here the three cases are one case, and the count is data rather than a branch:
//!
//! | | participants | remote participants | contacts involved |
//! |---|---|---|---|
//! | notes-to-self | `{me}` | none | none |
//! | DM | `{me, them}` | `{them}` | one |
//! | group | `{me, a, b, …}` | `{a, b, …}` | N |
//!
//! "Delivered by definition" stops being a special case: it is what *delivered to all zero remote participants* evaluates to. Presence is derived from the remote participants, so a conversation with none has nothing to ping and no offline state to show. Nothing here asks which case it is in, and nothing caps N — scale is bounded by the device's memory and CPU, never by the model.
//!
//! The crypto layer was always ready for this: `FriendshipId::derive` and `derive_conversation_token` both sort a participant list and hash it, and `FriendshipChains` already holds one chain per participant. Only the layer above it insisted on two.

use crate::types::friendship::FriendshipId;
use crate::types::{ChatMessage, HistoryRecovery};

/// A participant's PARTY ID — their pinned identity pubkey, the same value `Contact::handle_hash` carries. Never a handle string: the string derives the identity seed, so storing it anywhere re-creates the honeypot (docs/identity-profile.md).
pub type PartyId = [u8; 32];

/// Stable identity of a conversation, derived from its sorted participant set. Reuses `FriendshipId::derive` unchanged — one derivation for the wire, local storage, and the UI, so the three can never disagree about which conversation this is.
pub type ConversationId = FriendshipId;

/// A conversation: who is in it, and what was said.
///
/// Deliberately holds NOTHING about people. A participant's name, avatar, address and presence live on their `Contact`; a conversation only knows the ids. That separation is what lets a conversation exist with zero contacts (notes-to-self) or many (a group) without changing shape.
#[derive(Clone, Debug)]
pub struct Conversation {
    /// Sorted, deduplicated party ids — INCLUDING our own. Sorted because the id derives from it and both sides must agree; deduplicated because a participant set is a set, and the old code expressed self-notes as `[our_pid, our_pid]`, which would make a binary search over the participant vec ambiguous.
    participants: Vec<PartyId>,
    /// Cached id — `participants` is immutable after construction, so this can never drift from it.
    id: ConversationId,
    /// Messages, oldest first.
    pub messages: Vec<ChatMessage>,
    /// Count of real inbound rows that landed while this conversation was NOT front-of-eyes. Drives the contacts-list unread treatment (inner coloured ring + heavier name + float-to-top — never a count glyph). Cleared and re-persisted the moment the conversation becomes the active view.
    pub unread_count: u32,
    /// Scroll position, in pixels from the bottom. Lives here rather than on a contact because it is a property of the place, not of a person. Runtime-only.
    pub scroll_offset: f32,
    /// Friend-assisted history recovery state machine (newest-first cursor pagination from a participant's copy). `None` = no recovery running/known. Runtime struct; the durable cursor + complete flag persist in conversation state.
    pub history_recovery: Option<HistoryRecovery>,
    /// Cached anti-entropy digest `(count, digest)` — invalidated (set `None`) on any message-set mutation and recomputed lazily by `anti_entropy_digest`. Runtime-only: it is recomputed on load. Stops the digest being re-folded over EVERY row on every sync-record build (it was an O(rows) blake3 pass per call, on the render thread).
    digest_cache: Option<(u32, [u8; 32])>,
}

impl Conversation {
    /// Build a conversation from any participant set. Sorts and deduplicates, so callers may pass ids in any order and need not know whether they are describing one participant or a thousand.
    pub fn new(participants: impl IntoIterator<Item = PartyId>) -> Self {
        let mut participants: Vec<PartyId> = participants.into_iter().collect();
        participants.sort_unstable();
        participants.dedup();
        let id = FriendshipId::derive(&participants);
        Self {
            participants,
            id,
            messages: Vec::new(),
            unread_count: 0,
            scroll_offset: 0.0,
            history_recovery: None,
            digest_cache: None,
        }
    }

    /// The anti-entropy digest `(count, digest)` over the syncable rows, ORDER-DEPENDENT and sorted by eagle_time. `digest = rolling H(prev ‖ H(timestamp ‖ H(content)))` walking rows oldest-first (the order `insert_message_sorted` maintains). Order matters ON PURPOSE: two sides holding the same messages in the same sequence hash identically; a mismatch means one side is MISSING or has REORDERED a message — which an order-free XOR fold would have hidden (its whole point was to reveal exactly that). Probe/control rows and tombstones are excluded (they never sync / carry no content to compare). Cached; recomputed only after a mutation invalidates it.
    pub fn anti_entropy_digest(&mut self) -> (u32, [u8; 32]) {
        if let Some(cached) = self.digest_cache {
            return cached;
        }
        let mut rolling = [0u8; 32];
        let mut n: u32 = 0;
        for m in self
            .messages
            .iter()
            .filter(|m| !crate::types::is_control_content(&m.content) && !m.deleted)
        {
            let row = blake3::Hasher::new()
                .update(&m.timestamp.to_le_bytes())
                .update(blake3::hash(m.content.as_bytes()).as_bytes())
                .finalize();
            rolling = *blake3::Hasher::new()
                .update(&rolling)
                .update(row.as_bytes())
                .finalize()
                .as_bytes();
            n += 1;
        }
        self.digest_cache = Some((n, rolling));
        (n, rolling)
    }

    /// Invalidate the cached digest — called on every path that changes the syncable row set (insert, tombstone). Cheap; the next `anti_entropy_digest` recomputes.
    pub fn invalidate_digest(&mut self) {
        self.digest_cache = None;
    }

    /// The newest live edit row targeting `target_ts` → (edit row's ts, new body). Render-time supersede: the original row is braid key material and never mutates, so "the current text" is a question about edit rows, answered newest-wins. A deleted edit row stops counting — deleting an edit reverts to the previous edit or the original.
    pub fn latest_edit_for(&self, target_ts: i64) -> Option<(i64, String)> {
        self.messages
            .iter()
            .rev()
            .filter(|m| !m.deleted)
            .find(|m| m.reference == Some((crate::types::RefKind::Edit, target_ts)))
            .map(|m| (m.timestamp, m.content.clone()))
    }

    /// The CURRENT reaction from one side of the conversation on the row at `target_ts` — the newest live reaction row in that direction wins; an empty glyph (empty content) is the retract, reading as none. Per-sender is per-direction until group rows carry real sender attribution.
    pub fn current_reaction(&self, target_ts: i64, from_outgoing: bool) -> Option<String> {
        self.messages
            .iter()
            .rev()
            .filter(|m| !m.deleted && m.is_outgoing == from_outgoing)
            .find(|m| m.reference == Some((crate::types::RefKind::React, target_ts)))
            .map(|m| m.content.clone())
            .filter(|g| !g.is_empty())
    }

    /// This conversation's stable id.
    pub fn id(&self) -> ConversationId {
        self.id
    }

    /// Everyone in it, ourselves included, in canonical order.
    pub fn participants(&self) -> &[PartyId] {
        &self.participants
    }

    /// Everyone EXCEPT us — the people a message actually has to reach.
    ///
    /// This is the method that replaces `is_self`. A send iterates it: for notes-to-self it yields nothing and the loop simply does not run, so the message is delivered because there was nobody to deliver it to. No branch, no flag, no forced field.
    pub fn remote_participants<'a>(&'a self, us: &'a PartyId) -> impl Iterator<Item = &'a PartyId> {
        self.participants.iter().filter(move |p| *p != us)
    }

    /// How many people other than us are in here. `0` = notes-to-self, `1` = a DM, more = a group — as an observation, never a mode.
    pub fn remote_count(&self, us: &PartyId) -> usize {
        self.remote_participants(us).count()
    }

    /// Is this party in this conversation?
    pub fn includes(&self, party: &PartyId) -> bool {
        self.participants.binary_search(party).is_ok()
    }

    /// Position of a participant in the canonical order — the index into `FriendshipChains`' per-participant vectors. `None` if they are not in this conversation.
    pub fn participant_index(&self, party: &PartyId) -> Option<usize> {
        self.participants.binary_search(party).ok()
    }

    /// Insert preserving timestamp order, upgrading a friend-recovered copy in place when the same row arrives over the wire. Mirrors `Contact::insert_message_sorted`, which this replaces.
    pub fn insert_message_sorted(&mut self, msg: ChatMessage) {
        // Any insert or in-place upgrade can change the syncable set (a new row, or a deleted-flag flip below), so drop the cached anti-entropy digest — recomputed lazily on the next request.
        self.digest_cache = None;
        // IDENTITY = (timestamp, content): the eagle_time and the bare text, never the metadata. One message reaches a device by several routes — the live wire frame, a sibling fleet-forward, a history-recovery page — and those copies differ ONLY in metadata (delivered / recovered / ack_hash; the live frame carries a real ack_hash a forward lacks). Keying dedup on anything else let two copies of one message coexist: the mac's duplicated message (2026-08-08) was a sibling fleet-forward (stored recovered=false) plus the live frame, which the old recovered-only collapse never merged. On a match, upgrade the surviving row's metadata monotonically and drop the duplicate.
        if let Some(existing) = self
            .messages
            .iter_mut()
            .find(|m| m.timestamp == msg.timestamp && m.content == msg.content)
        {
            existing.delivered |= msg.delivered;
            existing.deleted |= msg.deleted;
            // Alert duty is fleet-monotone: once ANY device discharged (or suppressed-as-clearer), every copy stays discharged.
            existing.notified |= msg.notified;
            if existing.ack_hash.is_none() {
                existing.ack_hash = msg.ack_hash;
            }
            // Reference is origin-written-once row identity; a copy that lost it (a route that predates the field) never un-sets it.
            if existing.reference.is_none() {
                existing.reference = msg.reference;
            }
            // A row witnessed live on the wire supersedes a friend-attested recovered copy.
            existing.recovered = existing.recovered && msg.recovered;
            return;
        }
        // A recovered PLACEHOLDER at this timestamp yields to an authoritative (live/witnessed) row even if the text differs — what we saw on the wire outranks friend-attested content.
        if !msg.recovered {
            if let Some(existing) = self
                .messages
                .iter_mut()
                .find(|m| m.timestamp == msg.timestamp && m.recovered)
            {
                *existing = msg;
                return;
            }
        }
        // TOTAL order = (timestamp, blake3(content)) — the row-identity fields and nothing else, THE SAME order the storage key encodes (message_row_key: BE time ‖ hash[..8]). Timestamp alone left same-tick rows in ARRIVAL order, which differs per device — two devices holding identical rows rendered different orders AND computed different anti-entropy digests (the rolling hash is order-dependent), re-firing the history walk forever between converged copies. The hash tiebreak runs only on equal timestamps (cross-sender same-tick — within one sender's 704ps stream ties are impossible); equal (timestamp, content) is the dedup branch above and never reaches here.
        let pos = self
            .messages
            .binary_search_by(|m| {
                m.timestamp.cmp(&msg.timestamp).then_with(|| {
                    blake3::hash(m.content.as_bytes())
                        .as_bytes()
                        .cmp(blake3::hash(msg.content.as_bytes()).as_bytes())
                })
            })
            .unwrap_or_else(|pos| pos);
        self.messages.insert(pos, msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(b: u8) -> PartyId {
        [b; 32]
    }

    /// One participant, two, or many — the same constructor, and an id that depends only on WHO is present.
    #[test]
    fn id_is_the_participant_set_regardless_of_order() {
        let a = Conversation::new([pid(3), pid(1), pid(2)]);
        let b = Conversation::new([pid(1), pid(2), pid(3)]);
        assert_eq!(
            a.id(),
            b.id(),
            "order must not change which conversation this is"
        );
        assert_eq!(a.participants(), b.participants());

        // Distinct sets are distinct conversations.
        assert_ne!(a.id(), Conversation::new([pid(1), pid(2)]).id());
    }

    /// Notes-to-self is a ONE-participant set, not `[me, me]`. The old code expressed it as a duplicate pair, which would make a binary search over the participant vec ambiguous — and gave it the id of a two-party conversation with itself.
    #[test]
    fn self_notes_is_one_participant_not_a_duplicated_pair() {
        let me = pid(7);
        let notes = Conversation::new([me, me]);
        assert_eq!(
            notes.participants(),
            &[me],
            "a set does not hold duplicates"
        );
        assert_eq!(notes.id(), Conversation::new([me]).id());
        assert_eq!(notes.remote_count(&me), 0);
        assert_eq!(notes.remote_participants(&me).count(), 0);
    }

    /// Supersede-by-reference: the newest edit row targeting a ts wins, a deleted edit reverts to the previous one, and the ORIGINAL row's content never changes (it is braid key material — strands resolve stored content by eagle_time, so mutating it would fork the chain).
    #[test]
    fn latest_edit_wins_and_deleting_an_edit_reverts() {
        let mut conv = Conversation::new([pid(1), pid(2)]);
        conv.insert_message_sorted(crate::types::ChatMessage::new_with_timestamp(
            "orignal".to_string(),
            true,
            100,
        ));
        assert_eq!(conv.latest_edit_for(100), None);

        conv.insert_message_sorted(
            crate::types::ChatMessage::new_with_timestamp("original".to_string(), true, 200)
                .with_reference(crate::types::RefKind::Edit, 100),
        );
        conv.insert_message_sorted(
            crate::types::ChatMessage::new_with_timestamp("original, truly".to_string(), true, 300)
                .with_reference(crate::types::RefKind::Edit, 100),
        );
        assert_eq!(
            conv.latest_edit_for(100),
            Some((300, "original, truly".to_string()))
        );

        // Deleting the newest edit reverts to the previous one; the original row is untouched throughout.
        conv.messages
            .iter_mut()
            .find(|m| m.timestamp == 300)
            .unwrap()
            .deleted = true;
        assert_eq!(
            conv.latest_edit_for(100),
            Some((200, "original".to_string()))
        );
        assert_eq!(
            conv.messages
                .iter()
                .find(|m| m.timestamp == 100)
                .unwrap()
                .content,
            "orignal"
        );
    }

    /// Reactions: newest-per-sender wins, empty retracts, replacing works, and the other party's slot is independent.
    #[test]
    fn current_reaction_is_newest_per_sender_and_empty_retracts() {
        let mut conv = Conversation::new([pid(1), pid(2)]);
        conv.insert_message_sorted(crate::types::ChatMessage::new_with_timestamp(
            "hello".to_string(),
            false,
            100,
        ));
        assert_eq!(conv.current_reaction(100, true), None);

        conv.insert_message_sorted(
            crate::types::ChatMessage::new_with_timestamp("\u{1F44D}".to_string(), true, 200)
                .with_reference(crate::types::RefKind::React, 100),
        );
        assert_eq!(
            conv.current_reaction(100, true),
            Some("\u{1F44D}".to_string())
        );
        assert_eq!(
            conv.current_reaction(100, false),
            None,
            "their slot is theirs"
        );

        // Replace, then retract (empty content).
        conv.insert_message_sorted(
            crate::types::ChatMessage::new_with_timestamp("\u{2764}".to_string(), true, 300)
                .with_reference(crate::types::RefKind::React, 100),
        );
        assert_eq!(
            conv.current_reaction(100, true),
            Some("\u{2764}".to_string())
        );
        conv.insert_message_sorted(
            crate::types::ChatMessage::new_with_timestamp(String::new(), true, 400)
                .with_reference(crate::types::RefKind::React, 100),
        );
        assert_eq!(conv.current_reaction(100, true), None);
    }

    /// The property the whole refactor rests on: "who must this reach" answers correctly for 0, 1 and N without being told which case it is.
    #[test]
    fn remote_participants_scales_from_none_to_many() {
        let me = pid(1);
        assert_eq!(Conversation::new([me]).remote_count(&me), 0);
        assert_eq!(Conversation::new([me, pid(2)]).remote_count(&me), 1);

        let group = Conversation::new((1u8..=50).map(pid));
        assert_eq!(group.remote_count(&me), 49);
        assert!(group.remote_participants(&me).all(|p| *p != me));
    }

    /// Participant index is the index into FriendshipChains' per-participant vectors, so it must agree with the canonical sort.
    #[test]
    fn participant_index_follows_canonical_order() {
        let c = Conversation::new([pid(9), pid(3), pid(5)]);
        assert_eq!(c.participant_index(&pid(3)), Some(0));
        assert_eq!(c.participant_index(&pid(5)), Some(1));
        assert_eq!(c.participant_index(&pid(9)), Some(2));
        assert_eq!(c.participant_index(&pid(4)), None);
        assert!(c.includes(&pid(5)) && !c.includes(&pid(4)));
    }

    /// REGRESSION (mac dupe, 2026-08-08): one message reaching a device by two routes — a sibling fleet-forward (recovered=false, no ack_hash) and then the live wire frame (recovered=false, with an ack_hash) — must collapse to ONE row keyed on (timestamp, content), upgrading metadata. The old recovered-only collapse left both, because neither copy was `recovered`.
    #[test]
    fn same_timestamp_and_content_collapses_across_routes() {
        use crate::types::ChatMessage;
        let mut c = Conversation::new([pid(1), pid(2)]);
        // Route 1: sibling fleet-forward — a plain non-recovered incoming row, no ACK yet.
        c.insert_message_sorted(ChatMessage::new_with_timestamp("hi".into(), false, 1000));
        // Route 2: the live wire frame for the SAME message — carries the real ack_hash.
        c.insert_message_sorted(
            ChatMessage::new_with_timestamp("hi".into(), false, 1000).with_ack_hash([7u8; 32]),
        );
        assert_eq!(
            c.messages.len(),
            1,
            "the two routes are one message — must not duplicate"
        );
        assert_eq!(
            c.messages[0].ack_hash,
            Some([7u8; 32]),
            "the live frame's ack_hash is adopted"
        );

        // A different text at the same tick is a different message (astronomically rare, but not the same row).
        c.insert_message_sorted(ChatMessage::new_with_timestamp("yo".into(), false, 1000));
        assert_eq!(
            c.messages.len(),
            2,
            "distinct content at one tick stays distinct"
        );

        // A recovered placeholder is superseded by a live row even when the text differs.
        let mut c2 = Conversation::new([pid(1), pid(2)]);
        let mut ph = ChatMessage::new_with_timestamp("placeholder".into(), false, 2000);
        ph.recovered = true;
        c2.insert_message_sorted(ph);
        c2.insert_message_sorted(ChatMessage::new_with_timestamp(
            "the real text".into(),
            false,
            2000,
        ));
        assert_eq!(
            c2.messages.len(),
            1,
            "recovered placeholder replaced, not duplicated"
        );
        assert_eq!(c2.messages[0].content, "the real text");
        assert!(!c2.messages[0].recovered);
    }

    /// The anti-entropy digest is ORDER-DEPENDENT (a rolling hash), so a missing OR reordered message shows as a mismatch — the property the old order-free XOR fold destroyed. It is also cached: recomputed only after a mutation.
    #[test]
    fn digest_is_order_dependent_and_cached() {
        use crate::types::ChatMessage;
        let m = |t: i64, s: &str| ChatMessage::new_with_timestamp(s.into(), false, t);

        // Same messages, same order → identical digest (two sides agree).
        let mut a = Conversation::new([pid(1), pid(2)]);
        let mut b = Conversation::new([pid(2), pid(1)]);
        for (t, s) in [(10, "one"), (20, "two"), (30, "three")] {
            a.insert_message_sorted(m(t, s));
            b.insert_message_sorted(m(t, s));
        }
        assert_eq!(
            a.anti_entropy_digest(),
            b.anti_entropy_digest(),
            "same set + order agree"
        );

        // MISSING a message → different digest (the whole point).
        let mut c = Conversation::new([pid(1), pid(2)]);
        c.insert_message_sorted(m(10, "one"));
        c.insert_message_sorted(m(30, "three"));
        assert_ne!(
            a.anti_entropy_digest(),
            c.anti_entropy_digest(),
            "a missing message must mismatch"
        );

        // Same three texts at DIFFERENT eagle_times = a different sequence → different digest (an XOR fold of (ts,content) would also differ here, but the rolling hash also catches a pure reorder that a content-set fold cannot).
        let mut d = Conversation::new([pid(1), pid(2)]);
        for (t, s) in [(11, "one"), (21, "two"), (31, "three")] {
            d.insert_message_sorted(m(t, s));
        }
        assert_ne!(
            a.anti_entropy_digest(),
            d.anti_entropy_digest(),
            "different sequence must mismatch"
        );

        // Count is the non-deleted syncable rows.
        assert_eq!(a.anti_entropy_digest().0, 3);

        // Cache: a second call without mutation returns the same value; a new message invalidates it.
        let first = a.anti_entropy_digest();
        assert_eq!(
            a.anti_entropy_digest(),
            first,
            "cached, stable without mutation"
        );
        a.insert_message_sorted(m(40, "four"));
        assert_ne!(
            a.anti_entropy_digest(),
            first,
            "a new message invalidates the cache"
        );
        assert_eq!(a.anti_entropy_digest().0, 4);
    }
}
