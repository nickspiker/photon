//! The conversation panel's behaviour — ONE panel for every conversation at any size (docs/conversations.md Phase 1): opening it from the orb, leaving it (Back / Escape), and its taps. The render arm lives in render.rs and stamps the ids this file reads.

use super::*;
use crate::ui::state::ConversationPage;

/// Pill and row slots under `conv_panel_btn_base` (the render arm stamps the same numbers).
const SLOT_LEAVE: HitId = 0;
const SLOT_MUTE: HitId = 1;
const SLOT_RENAME: HitId = 2;
const SLOT_BOOT: HitId = 3;
const SLOT_CLEAR_WAVES: HitId = 4;
const SLOT_CLEAR_ALL: HitId = 5;
const SLOT_MESSAGE: HitId = 6;
const SLOT_BONDS: HitId = 8;
/// The slot span reserved in init: the pills, then 40 Bonds rows.
pub(super) const CONV_PANEL_SLOTS: HitId = 48;

impl PhotonApp {
    /// The one other person the open conversation is with — None for a group, our own notes, or a sibling's row. Such a conversation's panel IS that person's page (Nick 2026-11-01: "the person management page is distinct from the one in a conversation, can we combine those?").
    pub(super) fn pair_person(&self) -> Option<[u8; 32]> {
        if self.active_molecule().is_some() {
            return None;
        }
        let c = self.contacts.get(self.active_contact()?)?;
        if c.is_sibling || self.our_party_id(c).is_none_or(|us| c.remote_count(&us) == 0) {
            return None;
        }
        Some(c.handle_hash)
    }

    /// The rail's pages for the open conversation: a two-person conversation's are the person (About) and its numbers; every other conversation's are Bonds · In numbers · Manage. The render arm draws this list and the tap reads it back by index.
    pub(super) fn conversation_rail(&self) -> Vec<ConversationPage> {
        match self.pair_person() {
            Some(p) => vec![ConversationPage::Person(p), ConversationPage::Stats],
            None => ConversationPage::RAIL.to_vec(),
        }
    }

    /// The orb inside a conversation opens its panel: a two-person conversation on the person's own page, any other on Bonds.
    pub(super) fn open_conversation_panel(&mut self, text: &mut fluor::text::TextRenderer) -> bool {
        if self.active_conversation.is_none() {
            return false;
        }
        self.change_focus(None);
        self.disarm_conversation_panel();
        self.settings_content_scroll = 0.0;
        if let Some(gi) = self.active_molecule() {
            let title = self.molecule_rosters[gi].1.title();
            if let Some(tb) = self.molecule_title_textbox.as_mut() {
                tb.set_text(&title, text);
            }
        }
        self.person_return = PersonReturn::Conversation;
        self.state = AppState::ConversationPanel(self.conversation_rail()[0]);
        true
    }

    /// Every two-tap arm drops (event-shown, interaction-cleared).
    fn disarm_conversation_panel(&mut self) {
        self.contact_boot_armed = false;
        self.contact_clear_armed = None;
        self.molecule_leave_armed = false;
    }

    /// Show one page of the panel, fresh from its top (the settings rule).
    fn goto_conversation_page(&mut self, page: ConversationPage) {
        // A group member's page reached from Bonds returns to Bonds only while it is open; any other page of a group's panel returns to the conversation. A two-person conversation's panel keeps where it was opened from across its pages (About and In numbers are both the person's).
        if self.person_return == PersonReturn::Bonds {
            self.person_return = PersonReturn::Conversation;
        }
        self.change_focus(None);
        self.disarm_conversation_panel();
        self.settings_content_scroll = 0.0;
        self.state = AppState::ConversationPanel(page);
    }

    /// THE PERSON, from the People list (docs/conversations.md D8, revised 2026-10-29: "people shows atoms and molecules"): their page — who they are, what they share, Message, Boot — instead of the conversation itself.
    /// The page draws inside the panel, which reads the active conversation, so the pair conversation is held active WITHOUT the open's side effects: no clearer claim, no unread cleared — looking at a person is not reading your conversation with them.
    pub(super) fn open_person_page(&mut self, ci: usize) {
        let Some(id) = self.contacts.get(ci).and_then(|c| self.our_party_id(c).map(|us| c.conversation(&us).id())) else {
            return;
        };
        if !self.ensure_conversation(id) {
            return;
        }
        self.set_active_conversation(Some(id));
        // Looking at someone makes their presence matter now, as entering a conversation does.
        self.contacts[ci].ping_backoff = 0;
        self.contacts[ci].last_pinged = None;
        self.ping_contact(ci);
        self.spawn_avatar_download(ci);
        let party = self.contacts[ci].handle_hash;
        self.goto_conversation_page(ConversationPage::Person(party));
        self.person_return = PersonReturn::Ready;
    }

    /// Back / Escape returns to where the page was opened from: the People list (nothing read), a group's Bonds, or the conversation, which is front-of-eyes again (clear any unread that slipped in).
    pub(super) fn leave_conversation_panel(&mut self) {
        match self.person_return {
            PersonReturn::Ready => {
                self.person_return = PersonReturn::Conversation;
                self.change_focus(None);
                self.disarm_conversation_panel();
                self.state = AppState::Ready;
                self.set_active_conversation(None);
                return;
            }
            PersonReturn::Bonds if matches!(self.state, AppState::ConversationPanel(ConversationPage::Person(_))) => {
                self.goto_conversation_page(ConversationPage::Bonds);
                return;
            }
            _ => {}
        }
        self.change_focus(None);
        self.disarm_conversation_panel();
        self.state = AppState::Conversation;
        if let Some(id) = self.active_conversation {
            self.clear_unread_of(id);
        }
        self.reset_contact_ping_backoff();
    }

    /// A press inside the panel: the rail switches page, the pills act, a Bonds row opens a person or toggles a staged invite. Returns whether the press was the panel's.
    pub(super) fn on_conversation_panel_hit(&mut self, hit_id: HitId) -> bool {
        let AppState::ConversationPanel(page) = self.state else {
            return false;
        };
        let base = self.conv_panel_btn_base;
        // Any press other than the armed pill disarms it.
        if self.contact_boot_armed && hit_id != base + SLOT_BOOT {
            self.contact_boot_armed = false;
            self.scene_dirty = true;
        }
        if let Some(all) = self.contact_clear_armed {
            if hit_id != base + if all { SLOT_CLEAR_ALL } else { SLOT_CLEAR_WAVES } {
                self.contact_clear_armed = None;
                self.scene_dirty = true;
            }
        }
        if self.molecule_leave_armed && hit_id != base + SLOT_LEAVE {
            self.molecule_leave_armed = false;
            self.scene_dirty = true;
        }
        let rail = self.conversation_rail();
        if self.conv_nav_base != HIT_NONE && hit_id >= self.conv_nav_base && hit_id < self.conv_nav_base + rail.len() as HitId {
            let p = rail[(hit_id - self.conv_nav_base) as usize];
            self.goto_conversation_page(p);
            return true;
        }
        if base == HIT_NONE || hit_id < base || hit_id >= base + CONV_PANEL_SLOTS {
            return false;
        }
        let gid = self.active_molecule().map(|gi| self.molecule_rosters[gi].0);
        match hit_id - base {
            SLOT_LEAVE => {
                if !self.molecule_leave_armed {
                    self.molecule_leave_armed = true;
                } else {
                    self.molecule_leave_armed = false;
                    // Leaving means gone from the list: back to the contacts, nothing to look at here any more. A conversation with only us in it that is still a contact row leaves by removing that row.
                    let left = match gid {
                        Some(gid) => self.leave_molecule(gid),
                        None => {
                            self.boot_active_contact();
                            true
                        }
                    };
                    if left {
                        self.change_focus(None);
                        self.set_active_conversation(None);
                        self.state = AppState::Ready;
                    }
                }
            }
            SLOT_MUTE => {
                if let Some(gid) = gid {
                    self.toggle_molecule_mute(gid);
                }
            }
            SLOT_RENAME => {
                if let Some(gid) = gid {
                    let title: String = self.molecule_title_textbox.as_ref().map(|t| t.chars.iter().collect()).unwrap_or_default();
                    self.rename_molecule(gid, &title);
                    self.change_focus(None);
                }
            }
            SLOT_MESSAGE => {
                // Message: the conversation of exactly the two of you, founded over the bond when there is none yet (the tap a person row used to make).
                let ConversationPage::Person(party) = page else {
                    return true;
                };
                // Already the conversation of the two of you, opened from it: Message goes back to it.
                if self.person_return == PersonReturn::Conversation && self.pair_person() == Some(party) {
                    self.leave_conversation_panel();
                    return true;
                }
                if let Some(ci) = self.contacts.iter().position(|c| c.handle_hash == party && !c.is_sibling) {
                    self.person_return = PersonReturn::Conversation;
                    self.disarm_conversation_panel();
                    if self.contacts[ci].friendship_id.is_none() {
                        self.found_pair_conversation(ci);
                    }
                    self.enter_pair_conversation(ci);
                }
            }
            SLOT_BOOT => {
                // Boot (two-tap) the person this page shows: first press arms, second fires. Removal is unilateral and local-plus-fleet only — ostracism, not erasure.
                let ConversationPage::Person(party) = page else {
                    return true;
                };
                if !self.contact_boot_armed {
                    self.contact_boot_armed = true;
                } else {
                    self.contact_boot_armed = false;
                    if let Some(ci) = self.contacts.iter().position(|c| c.handle_hash == party && !c.is_sibling) {
                        let was_open = self.active_contact() == Some(ci) && gid.is_none();
                        if self.boot_contact(ci) {
                            if was_open {
                                // The conversation was the friendship (until pair conversations outlive their bonds, docs/conversations.md Phase 3): it went with them.
                                self.set_active_conversation(None);
                                self.state = AppState::Ready;
                            } else {
                                self.goto_conversation_page(ConversationPage::Bonds);
                            }
                        }
                    }
                }
            }
            slot @ (SLOT_CLEAR_WAVES | SLOT_CLEAR_ALL) => {
                // Clear waves or all history, two-tap like Boot: across our fleet, recordings discarded, the other members' copies untouched (manage.rs).
                let all = slot == SLOT_CLEAR_ALL;
                if self.contact_clear_armed == Some(all) {
                    self.contact_clear_armed = None;
                    self.clear_active_history(!all);
                } else {
                    self.contact_clear_armed = Some(all);
                }
            }
            // A BONDS row (Nick 2026-10-24): a member opens their Person page; a friend toggles a STAGED invite, and nothing is sent until the page is left (commit_bonds_if_left), so selecting and unselecting is free. A sent invite is settled — it cannot be undone (Nick 2026-11-02), so its row has no checkbox and takes no tap.
            k if k >= SLOT_BONDS => {
                let Some(row) = self.bond_rows.get((k - SLOT_BONDS) as usize).cloned() else {
                    return true;
                };
                match row.kind {
                    super::BondKind::You | super::BondKind::Member => {
                        self.goto_conversation_page(ConversationPage::Person(row.party));
                        self.person_return = PersonReturn::Bonds;
                    }
                    // A sent invite cannot be undone (Nick 2026-11-02) — its row takes no tap.
                    super::BondKind::Invited => {}
                    super::BondKind::Friend => {
                        if let Some(gid) = gid {
                            let key = (gid, row.party);
                            if let Some(i) = self.bonds_staged.iter().position(|s| *s == key) {
                                self.bonds_staged.remove(i);
                            } else {
                                self.bonds_staged.push(key);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        self.scene_dirty = true;
        true
    }
}
