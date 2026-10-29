//! IDEAS & FIXES — anonymous feedback from the field (Nick 2026-10-03: "an ideas/fixes page for people to submit anonymous feedback about Photon for improving. Since each report would have an ID, provenance hash or whatever's clever, they should be able to see their submitted and status on each too").
//!
//! A gripe carries NO identity: no handle, no device key, no signature — its text, its kind (idea or fix), and an id this device minted as blake3(text ‖ nonce) with a random nonce it keeps.
//! The worker re-derives the id from the text and nonce it receives and refuses a mismatch, so an id is provenance of its own text and nothing else; the worker sees the sender's address, as any HTTPS server does, and nothing more.
//! This device keeps the ids it sent (`gripes.mine`, device-local, a VSF document of multi-value fields) and asks the worker for each one's status when the page opens: received, seen, planned, fixed (with the version), declined, duplicate — with whatever note the developer left.
//! Every gripe is PUBLIC (Nick 2026-10-28): the page lists everyone's, ranked by likeness to what is in the box so a sender sees whether theirs is already in, and the ticker (device-local, off by default) raises a notification for each new one.
//! The developer answers with `photonlog --gripe-status` (the token in keys/); `photonlog --gripes` lists them all; see docs/ideas.md.

use super::*;

/// One gripe this device sent, with the last status it heard.
#[derive(Clone, Debug)]
pub(super) struct Gripe {
    pub id: [u8; 32],
    pub kind: String,
    pub text: String,
    pub osc: i64,
    pub state: String,
    pub note: String,
    pub version: String,
}

/// What the off-thread workers report back to the tick.
pub(super) enum GripeEvent {
    Put([u8; 32], Result<(), String>),
    Status([u8; 32], Result<(String, String, String), String>),
    /// The public list, fetched whole.
    List(Result<Vec<crate::network::fgtw::PublicGripe>, String>),
}

/// The device-local setting the sent ids live under.
const MINE_KEY: &str = "gripes.mine";
/// The device-local ticker switch (off when absent).
const TICKER_KEY: &str = "ideas.ticker";

/// The words a likeness match compares: lowercase runs of letters and digits, three or more characters long (shorter ones — "a", "to", "of" — match everything).
fn words_of(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .map(|w| w.to_lowercase())
        .collect();
    out.sort();
    out.dedup();
    out
}

fn encode_mine(mine: &[Gripe]) -> Option<Vec<u8>> {
    let mut sec = vsf::file_format::VsfSection::new("mine");
    sec.add_field_multi("id", mine.iter().map(|g| vsf::VsfType::v(b'g', g.id.to_vec())).collect());
    sec.add_field_multi("kind", mine.iter().map(|g| vsf::VsfType::a(g.kind.clone())).collect());
    sec.add_field_multi("text", mine.iter().map(|g| vsf::VsfType::v(b't', g.text.as_bytes().to_vec())).collect());
    sec.add_field_multi("osc", mine.iter().map(|g| vsf::VsfType::e(vsf::types::EtType::e6(g.osc))).collect());
    vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section_direct(sec)
        .build()
        .ok()
}

fn decode_mine(bytes: &[u8]) -> Vec<Gripe> {
    // Schema-validated (vsf trust gate): the document verifies whole before a field is read.
    let schema = vsf::schema::SectionSchema::new("mine")
        .field("id", vsf::schema::TypeConstraint::Wrapped(b'g'))
        .field("kind", vsf::schema::TypeConstraint::AsciiText)
        .field("text", vsf::schema::TypeConstraint::Wrapped(b't'))
        .field("osc", vsf::schema::TypeConstraint::AnyEagleTime);
    let Ok(sec) = vsf::schema::SectionBuilder::parse_document(schema, bytes, None) else { return Vec::new() };
    let vals = |name: &str| sec.get_values(name).unwrap_or_default();
    let (ids, kinds, texts, oscs) = (vals("id"), vals("kind"), vals("text"), vals("osc"));
    let mut out = Vec::new();
    for i in 0..ids.len() {
        let id = match ids.get(i) {
            Some(vsf::VsfType::v(_, b)) if b.len() == 32 => {
                let mut a = [0u8; 32];
                a.copy_from_slice(b);
                a
            }
            _ => continue,
        };
        let kind = match kinds.get(i) {
            Some(vsf::VsfType::a(s)) | Some(vsf::VsfType::d(s)) => s.clone(),
            _ => "idea".to_string(),
        };
        let text = match texts.get(i) {
            Some(vsf::VsfType::v(_, b)) => String::from_utf8_lossy(b).into_owned(),
            _ => String::new(),
        };
        let osc = match oscs.get(i) {
            Some(vsf::VsfType::e(vsf::types::EtType::e6(o))) => *o,
            Some(vsf::VsfType::e(vsf::types::EtType::e5(o))) => *o as i64,
            Some(vsf::VsfType::e(vsf::types::EtType::e7(o))) => *o as i64,
            _ => 0,
        };
        out.push(Gripe { id, kind, text, osc, state: String::new(), note: String::new(), version: String::new() });
    }
    out
}

impl PhotonApp {
    /// The sent ids, from the device-local setting; the statuses come back thru `refresh_gripe_states`.
    pub(super) fn load_my_gripes(&mut self) {
        let stored = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.device_local(MINE_KEY))
            .and_then(crate::storage::fleet_settings::as_bytes)
            .map(|b| decode_mine(&b))
            .unwrap_or_default();
        // Keep the statuses already heard this session for ids we still hold.
        let mut merged = stored;
        for g in merged.iter_mut() {
            if let Some(old) = self.my_gripes.iter().find(|o| o.id == g.id) {
                g.state = old.state.clone();
                g.note = old.note.clone();
                g.version = old.version.clone();
            }
        }
        self.my_gripes = merged;
    }

    fn save_my_gripes(&mut self) {
        let Some(bytes) = encode_mine(&self.my_gripes) else { return };
        let now = vsf::eagle_time_oscillations();
        if self.ensure_fleet_settings() {
            let fs = self.fleet_settings.as_mut().unwrap();
            if fs.linked(MINE_KEY) {
                fs.set_link(MINE_KEY, false, now);
            }
            if fs.set(MINE_KEY, vsf::VsfType::v(b'g', bytes), now) {
                self.persist_and_push_settings();
            }
        }
    }

    fn gripe_channel(&mut self) -> std::sync::mpsc::Sender<GripeEvent> {
        if self.gripe_tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.gripe_rx = Some(rx);
            self.gripe_tx = Some(tx);
        }
        self.gripe_tx.clone().unwrap()
    }

    /// The Idea / Fix pill: mint the id, remember it, send it off-thread, clear the box.
    pub(super) fn submit_gripe(&mut self, kind: &str) {
        let text: String = self
            .ideas_textbox
            .as_ref()
            .map(|tb| tb.chars.iter().collect::<String>())
            .unwrap_or_default()
            .trim()
            .to_string();
        if text.is_empty() {
            self.ready_toast = Some(tr(Msg::IdeasNothingTyped).into_owned());
            return;
        }
        let nonce: [u8; 32] = rand::random();
        let mut h = blake3::Hasher::new();
        h.update(text.as_bytes());
        h.update(&nonce);
        let id = *h.finalize().as_bytes();
        self.my_gripes.insert(
            0,
            Gripe { id, kind: kind.to_string(), text: text.clone(), osc: vsf::eagle_time_oscillations(), state: "sending".to_string(), note: String::new(), version: String::new() },
        );
        self.save_my_gripes();
        if let Some(tb) = self.ideas_textbox.as_mut() {
            tb.clear();
        }
        let tx = self.gripe_channel();
        let kind = kind.to_string();
        std::thread::spawn(move || {
            let r = crate::network::fgtw::gripe_put_blocking(&id, &nonce, &kind, &text).map_err(|e| e.to_string());
            let _ = tx.send(GripeEvent::Put(id, r));
        });
        self.ready_toast = Some(tr(Msg::IdeasSending).into_owned());
        self.scene_dirty = true;
    }

    /// Ask the worker for every sent id's status, off-thread; the tick folds the answers in.
    pub(super) fn refresh_gripe_states(&mut self) {
        let ids: Vec<[u8; 32]> = self.my_gripes.iter().map(|g| g.id).collect();
        if ids.is_empty() {
            return;
        }
        let tx = self.gripe_channel();
        std::thread::spawn(move || {
            for id in ids {
                let r = crate::network::fgtw::gripe_get_blocking(&id).map_err(|e| e.to_string());
                let _ = tx.send(GripeEvent::Status(id, r));
            }
        });
    }

    /// Fetch everyone's gripes, off-thread (the list is public): on the Ideas page's open, on the hub's `idea` edge, and when the ticker is switched on.
    pub(super) fn refresh_public_gripes(&mut self) {
        let tx = self.gripe_channel();
        std::thread::spawn(move || {
            let r = crate::network::fgtw::gripe_list_blocking().map_err(|e| e.to_string());
            let _ = tx.send(GripeEvent::List(r));
        });
    }

    /// A fetched list lands: with the ticker on, every gripe new since the last fetch (and not one this device sent) raises a notification; the first fetch of a session is the baseline and announces nothing.
    fn adopt_public_gripes(&mut self, rows: Vec<crate::network::fgtw::PublicGripe>) {
        if self.ideas_ticker && self.public_gripes_known {
            let fresh: Vec<&crate::network::fgtw::PublicGripe> = rows
                .iter()
                .filter(|g| !self.public_gripes.iter().any(|o| o.id == g.id) && !self.my_gripes.iter().any(|m| m.id == g.id))
                .collect();
            for g in fresh {
                let title = tr(if g.kind == "fix" { Msg::IdeasTickerNewFix } else { Msg::IdeasTickerNewIdea }).into_owned();
                crate::logf!("IDEAS: ticker — new {} {}", g.kind, hex::encode(&g.id[..4]));
                #[cfg(target_os = "android")]
                crate::platform::jni_android::notify_new_message(&g.id, &g.id, &title, &g.text, None);
                #[cfg(not(target_os = "android"))]
                crate::platform::desktop_notify::notify_new_message(&g.id, &title, &g.text, None);
            }
        }
        self.public_gripes = rows;
        self.public_gripes_known = true;
    }

    /// The ticker switch: device-local, default off; switching it on fetches the baseline now so the next idea is the first one announced.
    pub(super) fn set_ideas_ticker(&mut self, on: bool) {
        self.ideas_ticker = on;
        let now = vsf::eagle_time_oscillations();
        if self.ensure_fleet_settings() {
            let fs = self.fleet_settings.as_mut().unwrap();
            if fs.linked(TICKER_KEY) {
                fs.set_link(TICKER_KEY, false, now);
            }
            if fs.set(TICKER_KEY, vsf::VsfType::u0(on), now) {
                crate::logf!("SETTINGS: {} = {} (device-local)", TICKER_KEY, on);
                self.persist_and_push_settings();
            }
        }
        if on {
            self.refresh_public_gripes();
        }
    }

    /// The ticker setting, read at session start: absent = off.
    pub(super) fn load_ideas_ticker(&mut self) {
        self.ideas_ticker = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.device_local(TICKER_KEY))
            .and_then(crate::storage::fleet_settings::as_bool)
            .unwrap_or(false);
        if let Some(cb) = self.settings_ideas_ticker_check.as_mut() {
            cb.set_checked(self.ideas_ticker);
        }
        if self.ideas_ticker && !self.public_gripes_known {
            self.refresh_public_gripes();
        }
    }

    /// Everyone's gripes as the Ideas page lists them: with the box empty, newest first; with words in it, only the gripes sharing a word with it, the most shared first — so a sender sees whether their idea is already in the bucket before sending it.
    /// A free function over what it reads, so the render can call it while it holds the chrome mutably.
    pub(super) fn ranked_public_gripes<'a>(public: &'a [crate::network::fgtw::PublicGripe], typed: &str) -> Vec<&'a crate::network::fgtw::PublicGripe> {
        let query = words_of(typed);
        let mut scored: Vec<(usize, &crate::network::fgtw::PublicGripe)> = public
            .iter()
            .map(|g| {
                let theirs = words_of(&g.text);
                (query.iter().filter(|w| theirs.contains(w)).count(), g)
            })
            .filter(|(score, _)| query.is_empty() || *score > 0)
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.received_osc.cmp(&a.1.received_osc)));
        scored.into_iter().map(|(_, g)| g).collect()
    }

    /// The tick's drain: acks and statuses from the threads.
    pub(super) fn drain_gripe_events(&mut self) -> bool {
        let events: Vec<GripeEvent> = self.gripe_rx.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        if events.is_empty() {
            return false;
        }
        for ev in events {
            match ev {
                GripeEvent::Put(id, Ok(())) => {
                    if let Some(g) = self.my_gripes.iter_mut().find(|g| g.id == id) {
                        g.state = "received".to_string();
                    }
                    self.ready_toast = Some(tr(Msg::IdeasSent).into_owned());
                    crate::logf!("IDEAS: sent {}", hex::encode(&id[..4]));
                }
                GripeEvent::Put(id, Err(e)) => {
                    if let Some(g) = self.my_gripes.iter_mut().find(|g| g.id == id) {
                        g.state = "not sent".to_string();
                    }
                    self.ready_toast = Some(tr(Msg::IdeasSendFailed(&e)).into_owned());
                    crate::logf!("IDEAS: send failed: {}", e);
                }
                GripeEvent::Status(id, Ok((state, note, version))) => {
                    if let Some(g) = self.my_gripes.iter_mut().find(|g| g.id == id) {
                        g.state = state;
                        g.note = note;
                        g.version = version;
                    }
                }
                GripeEvent::Status(id, Err(e)) => {
                    crate::logf!("IDEAS: status of {} unavailable: {}", hex::encode(&id[..4]), e);
                }
                GripeEvent::List(Ok(rows)) => self.adopt_public_gripes(rows),
                GripeEvent::List(Err(e)) => {
                    crate::logf!("IDEAS: the public list is unavailable: {}", e);
                }
            }
        }
        self.scene_dirty = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gripe(n: u8, text: &str, osc: i64) -> crate::network::fgtw::PublicGripe {
        crate::network::fgtw::PublicGripe { id: [n; 32], kind: "idea".into(), state: "received".into(), received_osc: osc, text: text.into(), note: String::new(), version: String::new() }
    }

    #[test]
    fn words_skip_the_short_ones_and_fold_case() {
        assert_eq!(words_of("Dark mode, a toggle for DARK themes!"), vec!["dark", "for", "mode", "themes", "toggle"]);
    }

    #[test]
    fn an_empty_box_lists_everything_newest_first() {
        let all = [gripe(1, "older", 10), gripe(2, "newer", 20)];
        let ranked = PhotonApp::ranked_public_gripes(&all, "");
        assert_eq!(ranked.iter().map(|g| g.id[0]).collect::<Vec<_>>(), vec![2, 1]);
    }

    #[test]
    fn typing_keeps_only_shared_words_most_shared_first() {
        let all = [gripe(1, "a dark mode toggle", 10), gripe(2, "dark theme", 30), gripe(3, "faster startup", 40)];
        let ranked = PhotonApp::ranked_public_gripes(&all, "dark mode please");
        assert_eq!(ranked.iter().map(|g| g.id[0]).collect::<Vec<_>>(), vec![1, 2], "two shared words outrank one; nothing shared is left out");
    }
}
