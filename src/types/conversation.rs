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
        }
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
        if let Some(existing) = self
            .messages
            .iter_mut()
            .find(|m| m.timestamp == msg.timestamp && m.recovered && !msg.recovered)
        {
            *existing = msg;
            return;
        }
        let pos = self
            .messages
            .binary_search_by(|m| m.timestamp.cmp(&msg.timestamp))
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
        assert_eq!(a.id(), b.id(), "order must not change which conversation this is");
        assert_eq!(a.participants(), b.participants());

        // Distinct sets are distinct conversations.
        assert_ne!(a.id(), Conversation::new([pid(1), pid(2)]).id());
    }

    /// Notes-to-self is a ONE-participant set, not `[me, me]`. The old code expressed it as a duplicate pair, which would make a binary search over the participant vec ambiguous — and gave it the id of a two-party conversation with itself.
    #[test]
    fn self_notes_is_one_participant_not_a_duplicated_pair() {
        let me = pid(7);
        let notes = Conversation::new([me, me]);
        assert_eq!(notes.participants(), &[me], "a set does not hold duplicates");
        assert_eq!(notes.id(), Conversation::new([me]).id());
        assert_eq!(notes.remote_count(&me), 0);
        assert_eq!(notes.remote_participants(&me).count(), 0);
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
}
