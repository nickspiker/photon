//! IDEAS & FIXES — anonymous feedback from the field (Nick 2026-10-03: "an ideas/fixes page for people to submit anonymous feedback about Photon for improving. Since each report would have an ID, provenance hash or whatever's clever, they should be able to see their submitted and status on each too").
//!
//! A gripe carries NO identity: no handle, no device key, no signature — its text, its kind (idea or fix), and an id this device minted as blake3(text ‖ nonce) with a random nonce it keeps.
//! The worker re-derives the id from the text and nonce it receives and refuses a mismatch, so an id is provenance of its own text and nothing else; the worker sees the sender's address, as any HTTPS server does, and nothing more.
//! This device keeps the ids it sent (`gripes.mine`, device-local, a VSF document of multi-value fields) and asks the worker for each one's status when the page opens: received, seen, planned, fixed (with the version), declined, duplicate — with whatever note the developer left.
//! The developer reads and answers with `photonlog --gripes` / `--gripe-status` (the token in keys/); see docs/ideas.md.

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
}

/// The device-local setting the sent ids live under.
const MINE_KEY: &str = "gripes.mine";

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
            }
        }
        self.scene_dirty = true;
        true
    }
}
