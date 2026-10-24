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
const SLOT_BONDS: HitId = 8;
/// The slot span reserved in init: the pills, then 40 Bonds rows.
pub(super) const CONV_PANEL_SLOTS: HitId = 48;

impl PhotonApp {
    /// The orb inside a conversation opens its panel on Bonds — the same page for every conversation.
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
        self.state = AppState::ConversationPanel(ConversationPage::Bonds);
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
        self.change_focus(None);
        self.disarm_conversation_panel();
        self.settings_content_scroll = 0.0;
        self.state = AppState::ConversationPanel(page);
    }

    /// Back / Escape: a Person page returns to Bonds; any other page returns to the conversation, which is front-of-eyes again (clear any unread that slipped in).
    pub(super) fn leave_conversation_panel(&mut self) {
        if matches!(self.state, AppState::ConversationPanel(ConversationPage::Person(_))) {
            self.goto_conversation_page(ConversationPage::Bonds);
            return;
        }
        self.change_focus(None);
        self.disarm_conversation_panel();
        self.state = AppState::Conversation;
        if let Some(gi) = self.active_molecule() {
            self.clear_molecule_unread(gi);
        } else if let Some(ci) = self.active_contact() {
            self.reset_contact_ping_backoff();
            self.clear_unread(ci);
        }
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
        if self.conv_nav_base != HIT_NONE && hit_id >= self.conv_nav_base && hit_id < self.conv_nav_base + ConversationPage::RAIL.len() as HitId {
            let p = ConversationPage::RAIL[(hit_id - self.conv_nav_base) as usize];
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
            // A BONDS row (Nick 2026-10-24): a member opens their Person page; an invite or a friend toggles a STAGED edit — an invite for a friend, a withdrawal for an open invite. Nothing is sent until the page is left (commit_bonds_if_left), so selecting and unselecting is free.
            k if k >= SLOT_BONDS => {
                let Some(row) = self.bond_rows.get((k - SLOT_BONDS) as usize).cloned() else {
                    return true;
                };
                match row.kind {
                    super::BondKind::You | super::BondKind::Member => self.goto_conversation_page(ConversationPage::Person(row.party)),
                    super::BondKind::Invited | super::BondKind::Friend => {
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
