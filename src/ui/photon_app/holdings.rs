//! DEVICE-REMOVAL SAFETY (Nick 2026-10-17: "is there a way to let the user know they're removing a device that has the only copy and to wait for sync or offer other options?").
//!
//! Phase 2, the have-set exchange: arming Release lists the attachment originals (and their heads) this device actually holds, seals the list under the fleet key, and asks every live sibling which of them it LACKS.
//! An item is at risk when some hash of it that we hold is missing on every sibling that answered — holders are counted per item, never per device, so a watch that holds three photos vouches for those three and nothing else.
//! The Security page shows the result under the Release pill (checking, all safe, or what exists only here) plus the siblings that have not answered and when we last heard from them; the second tap still releases.
//! Phase 3, copy-then-release: one tap asks ONE sibling (the most complete holder that answered) to fetch the at-risk set from this device now, and persists `release.when_copied` (device-local).
//! The sibling fetches thru the ordinary attachment fetch, aimed at the leaver first, and re-answers as items land; when nothing is at risk any more the leaver sends its departure request by itself, and the approval words appear on its Security page as for any Release.
//! The flag survives a restart: with it set, the check re-runs as soon as the session and the fleet key are up, and a fresh pull goes out once the answers show what is still at risk.
//! The list carries RAW content hashes: presence follows the raw→hb alias, so a wrapped original answers the same as a raw one; the ask moves to header hb when the raw form retires at v134.
//! Every presence probe runs on a worker (the UI thread makes no vault call); the fleet key is read from RAM only.

use super::*;
use crate::network::fgtw::protocol::HoldingsKind;
use std::collections::{HashMap, HashSet};

/// The device-local setting that makes a departure wait for its copies.
const RELEASE_WHEN_COPIED: &str = "release.when_copied";

/// One attachment this device's rows point at: the original first, then its head when it has one.
pub(super) struct HeldItem {
    recording: bool,
    size: u64,
    hashes: Vec<[u8; 32]>,
}

/// The leaver's check in flight (or landed) — one per arm of the Release pill, or per launch while a copy-then-release is pending.
pub(super) struct HoldingsCheck {
    nonce: [u8; 32],
    items: Vec<HeldItem>,
    /// `None` while the presence worker runs; then the hashes this device actually holds.
    held: Option<HashSet<[u8; 32]>>,
    probe_rx: Option<std::sync::mpsc::Receiver<HashSet<[u8; 32]>>>,
    /// The sibling devices asked; an answer from anyone else is dropped.
    asked: Vec<[u8; 32]>,
    /// Per answering sibling: the hashes of our ask it does NOT hold.
    answers: HashMap<[u8; 32], HashSet<[u8; 32]>>,
    /// No fleet key in RAM, so no ask could be sealed.
    no_key: bool,
    /// The sealed ask as sent and when, for the ONE re-ask a silent sibling gets when we next hear from it (an edge, not a timer).
    ask_frame: Option<Vec<u8>>,
    asked_at: Option<std::time::Instant>,
    reasked: HashSet<[u8; 32]>,
    /// The copy in flight: the sibling asked to fetch, and the hashes asked for.
    pull: Option<([u8; 32], HashSet<[u8; 32]>)>,
}

/// A sibling's finished presence probe, handed from its worker back to the UI thread: an answer to seal and send, or (`pull`) the start of a fetch job.
pub(super) struct HoldingsServe {
    nonce: [u8; 32],
    to: [u8; 32],
    addr: std::net::SocketAddr,
    pull: bool,
    missing: Vec<[u8; 32]>,
}

/// SIBLING side of a pull: what a leaver asked us to fetch from it, and what has not landed yet. RAM only — a restart drops it, and the leaver's own restart re-pulls.
pub(super) struct PullJob {
    leaver: [u8; 32],
    addr: std::net::SocketAddr,
    nonce: [u8; 32],
    /// Fetches in flight.
    pending: HashSet<[u8; 32]>,
    /// Everything of the pull we still lack: the pending fetches plus any hash no row here points at (nothing to fetch it for).
    missing: HashSet<[u8; 32]>,
    total: usize,
    landed: usize,
}

/// What the Security page draws under the Release pill.
pub(super) enum HoldingsView {
    Checking,
    NoKey,
    Safe,
    AtRisk { files: u64, recordings: u64, bytes: u64 },
}

/// The verdict plus who it concerns: each asked sibling that has not answered ("name (age)"), and the sibling copying the at-risk set — or the one a copy would go to.
pub(super) struct HoldingsShown {
    pub view: HoldingsView,
    pub waiting: Vec<String>,
    pub target: Option<String>,
}

/// The seal key for every holdings frame: a domain-separated derivation of the fleet key, so a holdings list never shares a key with any other fleet ciphertext.
pub(super) fn holdings_key(fleet_key: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"PHOTON_FLEET_HOLDINGS_v\x01");
    h.update(fleet_key);
    *h.finalize().as_bytes()
}

/// A sealed list's plaintext as hashes; a length that is not a whole number of hashes is a malformed list and reads as none.
pub(super) fn split_hashes(plain: &[u8]) -> Option<Vec<[u8; 32]>> {
    if plain.len() % 32 != 0 {
        return None;
    }
    Some(plain.chunks_exact(32).map(|c| <[u8; 32]>::try_from(c).expect("chunks_exact(32)")).collect())
}

/// The items at risk: some hash of the item that we hold is missing on every sibling that answered. With nobody to ask, everything held is at risk.
fn at_risk<'a>(items: &'a [HeldItem], held: &HashSet<[u8; 32]>, answers: &HashMap<[u8; 32], HashSet<[u8; 32]>>) -> Vec<&'a HeldItem> {
    items
        .iter()
        .filter(|item| {
            item.hashes
                .iter()
                .any(|h| held.contains(h) && answers.values().all(|missing| missing.contains(h)))
        })
        .collect()
}

/// File an answer under its sibling. A pull target's answer covers only the pulled hashes, so it replaces just that part of what the sibling lacked; any other answer replaces the whole entry.
fn merge_answer(answers: &mut HashMap<[u8; 32], HashSet<[u8; 32]>>, from: [u8; 32], pulled: Option<&HashSet<[u8; 32]>>, missing: Vec<[u8; 32]>) {
    match (pulled, answers.get_mut(&from)) {
        (Some(pulled), Some(entry)) => {
            entry.retain(|h| !pulled.contains(h));
            entry.extend(missing);
        }
        _ => {
            answers.insert(from, missing.into_iter().collect());
        }
    }
}

/// The sibling a copy goes to: the most complete holder that answered (fewest missing), ties broken by key so every run picks the same one.
fn pull_target(answers: &HashMap<[u8; 32], HashSet<[u8; 32]>>) -> Option<[u8; 32]> {
    answers.iter().min_by_key(|(k, m)| (m.len(), **k)).map(|(k, _)| *k)
}

fn sibling_name(contacts: &[crate::types::Contact], device: &[u8; 32]) -> String {
    contacts
        .iter()
        .find(|c| c.is_sibling && c.knows_device(device))
        .map(|c| c.display_name())
        .unwrap_or_else(|| crate::fp(device))
}

impl PhotonApp {
    fn fleet_key_ram_only(&self) -> Option<[u8; 32]> {
        self.fleet_key_ram.lock().ok().and_then(|g| *g)
    }

    /// LEAVER, on arming Release (or at launch while a copy-then-release is pending): list every attachment the rows point at and hand the presence probe to a worker; the ask goes out when it lands ([`Self::drain_holdings`]).
    pub(super) fn holdings_check_start(&mut self) {
        let mut seen = HashSet::new();
        let mut items = Vec::new();
        for conv in &self.conversations {
            for m in &conv.messages {
                if m.deleted || m.is_control() {
                    continue;
                }
                let Some(f) = m.file.as_ref() else { continue };
                // A wave's envelope pyramid only draws the card; the recording itself is the WaveAudio row.
                if f.role == crate::types::AttachRole::WaveEnv || !seen.insert(f.hash) {
                    continue;
                }
                let mut hashes = vec![f.hash];
                hashes.extend(f.head);
                items.push(HeldItem { recording: f.role == crate::types::AttachRole::WaveAudio, size: f.size, hashes });
            }
        }
        let all: Vec<[u8; 32]> = items.iter().flat_map(|i| i.hashes.iter().copied()).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.event_proxy.clone();
        let _ = std::thread::Builder::new().name("holdings-probe".into()).spawn(move || {
            let held: HashSet<[u8; 32]> = all.into_iter().filter(|h| crate::storage::blob_present_probe_now(h)).collect();
            let _ = tx.send(held);
            if let Some(w) = wake {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });
        crate::logf!("HOLDINGS: check started over {} attachments", items.len());
        self.holdings_check = Some(HoldingsCheck {
            nonce: rand::random::<[u8; 32]>(),
            items,
            held: None,
            probe_rx: Some(rx),
            asked: Vec::new(),
            answers: HashMap::new(),
            no_key: false,
            ask_frame: None,
            asked_at: None,
            reasked: HashSet::new(),
            pull: None,
        });
    }

    /// The holdings edges, once per tick: the leaver's probe landing (send the ask), a sibling's finished probes (answer, or start a pull), a silent sibling heard from again (its one re-ask), and a pending copy-then-release after launch (start the check). True when the page should repaint.
    pub(super) fn drain_holdings(&mut self) -> bool {
        let mut changed = false;
        if self.release_when_copied
            && self.holdings_check.is_none()
            && self.depart_request_t.is_none()
            && self.session.is_some()
            && self.device_keypair.is_some()
            && self.fleet_key_ram_only().is_some()
        {
            crate::log("HOLDINGS: copy-then-release pending from before — checking again");
            self.holdings_check_start();
            changed = true;
        }
        let landed = self.holdings_check.as_ref().and_then(|c| c.probe_rx.as_ref()).and_then(|rx| rx.try_recv().ok());
        if let Some(held) = landed {
            self.holdings_send_ask(held);
            changed = true;
        }
        let served: Vec<HoldingsServe> = self.holdings_serve_rx.try_iter().collect();
        for s in served {
            if s.pull {
                self.holdings_start_pull_job(s);
            } else {
                self.holdings_send_missing(s.nonce, s.to, s.addr, &s.missing);
            }
        }
        // A sibling that never answered and has since spoken to us gets the ask once more — it may have been offline, or the ask lost.
        let reask: Vec<([u8; 32], Vec<u8>)> = match self.holdings_check.as_ref() {
            Some(HoldingsCheck { ask_frame: Some(frame), asked_at: Some(at), asked, answers, reasked, .. }) => asked
                .iter()
                .filter(|d| !answers.contains_key(*d) && !reasked.contains(*d))
                .filter(|d| self.contacts.iter().any(|c| c.is_sibling && c.knows_device(d) && c.last_heard.is_some_and(|h| h > *at)))
                .map(|d| (*d, frame.clone()))
                .collect(),
            _ => Vec::new(),
        };
        for (d, frame) in reask {
            crate::logf!("HOLDINGS: {} is back and never answered — asking once more", crate::fp(&d));
            self.dispatch_frame_to_sibling(frame, Some(d));
            if let Some(c) = self.holdings_check.as_mut() {
                c.reasked.insert(d);
            }
        }
        changed
    }

    fn holdings_send_ask(&mut self, held: HashSet<[u8; 32]>) {
        let key = self.fleet_key_ram_only();
        let me = self.device_keypair.as_ref().map(|kp| (kp.public.to_bytes(), *kp.secret.as_bytes()));
        let siblings: Vec<[u8; 32]> = self
            .contacts
            .iter()
            .filter(|c| c.is_sibling && !c.locked_out)
            .filter_map(|c| c.device_key())
            .collect();
        let Some(check) = self.holdings_check.as_mut() else { return };
        check.probe_rx = None;
        let mut list: Vec<[u8; 32]> = held.iter().copied().collect();
        list.sort_unstable();
        check.held = Some(held);
        if list.is_empty() {
            crate::log("HOLDINGS: this device holds no attachments — nothing to ask");
            self.holdings_release_if_safe();
            return;
        }
        let (Some(key), Some((pk, sk))) = (key, me) else {
            crate::log("HOLDINGS: no fleet key in RAM — cannot seal the ask");
            check.no_key = true;
            return;
        };
        let frame = kete::encrypt_bytes(&list.concat(), &holdings_key(&key))
            .and_then(|sealed| crate::network::fgtw::protocol::build_holdings_vsf(HoldingsKind::Ask, &check.nonce, sealed, &pk, &sk));
        match frame {
            Ok(frame) => {
                crate::logf!("HOLDINGS: asked {} siblings about {} held hashes", siblings.len(), list.len());
                check.asked = siblings;
                check.ask_frame = Some(frame.clone());
                check.asked_at = Some(std::time::Instant::now());
                self.dispatch_frame_to_sibling(frame, None);
            }
            Err(e) => {
                crate::logf!("HOLDINGS: ask build failed: {}", e);
                check.no_key = true;
            }
        }
    }

    /// SIBLING, on a `holdings_ask` or `holdings_pull` (sender already gated as a live member): open the list and probe it on a worker; [`Self::drain_holdings`] answers, or starts the fetches of a pull. A pull we are already working on just re-answers where it stands.
    pub(super) fn holdings_serve(&mut self, kind: HoldingsKind, nonce: [u8; 32], sealed: Vec<u8>, from: [u8; 32], addr: std::net::SocketAddr) {
        let pull = kind == HoldingsKind::Pull;
        if pull {
            if let Some(job) = self.holdings_pulls.iter().find(|j| j.leaver == from && j.nonce == nonce) {
                let missing: Vec<[u8; 32]> = job.missing.iter().copied().collect();
                self.holdings_send_missing(nonce, from, addr, &missing);
                return;
            }
        }
        let Some(key) = self.fleet_key_ram_only() else {
            crate::log("HOLDINGS: a sibling asked but no fleet key in RAM — unanswered");
            return;
        };
        let Some(asked) = kete::decrypt_bytes(&sealed, &holdings_key(&key)).ok().and_then(|p| split_hashes(&p)) else {
            crate::logf!("HOLDINGS: frame from {} did not open — dropped", crate::fp(&from));
            return;
        };
        let tx = self.holdings_serve_tx.clone();
        let wake = self.event_proxy.clone();
        let _ = std::thread::Builder::new().name("holdings-serve".into()).spawn(move || {
            let missing: Vec<[u8; 32]> = asked.into_iter().filter(|h| !crate::storage::blob_present_probe_now(h)).collect();
            let _ = tx.send(HoldingsServe { nonce, to: from, addr, pull, missing });
            if let Some(w) = wake {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });
    }

    fn holdings_send_missing(&self, nonce: [u8; 32], to: [u8; 32], addr: std::net::SocketAddr, missing: &[[u8; 32]]) {
        let (Some(key), Some(kp), Some(checker)) = (self.fleet_key_ram_only(), self.device_keypair.as_ref(), self.status_checker.as_ref()) else {
            return;
        };
        let frame = kete::encrypt_bytes(&missing.concat(), &holdings_key(&key)).and_then(|sealed| {
            crate::network::fgtw::protocol::build_holdings_vsf(HoldingsKind::Missing, &nonce, sealed, &kp.public.to_bytes(), kp.secret.as_bytes())
        });
        match frame {
            Ok(frame) => {
                crate::logf!("HOLDINGS: answered {} — missing {}", crate::fp(&to), missing.len());
                let _ = checker.history_dispatch().send(crate::network::status::HistorySendRequest {
                    peer_addr: addr,
                    alt_addr: None,
                    recipient_pubkey: to,
                    relay_to: vec![to],
                    vsf_bytes: frame,
                    tag: None,
                });
            }
            Err(e) => crate::logf!("HOLDINGS: answer build failed: {}", e),
        }
    }

    /// SIBLING: a pull's probe landed — fetch every missing hash a row here points at, from the leaver first ([`Self::attach_fetch`] ranks `attach_fetch_prefer` ahead of everyone), and answer at once with where we stand.
    fn holdings_start_pull_job(&mut self, s: HoldingsServe) {
        let mut row_of: HashMap<[u8; 32], usize> = HashMap::new();
        for ci in 0..self.contacts.len() {
            let Some(conv) = self.conv_of(ci) else { continue };
            for m in conv.messages.iter().filter(|m| !m.deleted) {
                if let Some(f) = m.file.as_ref() {
                    row_of.entry(f.hash).or_insert(ci);
                    if let Some(h) = f.head {
                        row_of.entry(h).or_insert(ci);
                    }
                }
            }
        }
        let mut job = PullJob { leaver: s.to, addr: s.addr, nonce: s.nonce, pending: HashSet::new(), missing: s.missing.iter().copied().collect(), total: 0, landed: 0 };
        for h in &s.missing {
            let Some(&sci) = row_of.get(h) else { continue };
            self.attach_fetch_prefer.insert(*h, s.to);
            // A fetch already rotating thru other candidates starts over, so its first ask goes to the leaver.
            self.attach_fetch_inflight.remove(h);
            job.pending.insert(*h);
            self.attach_fetch(sci, h);
        }
        job.total = job.pending.len();
        crate::logf!(
            "HOLDINGS: {} asked us to take {} items — fetching {} ({} have no row here)",
            crate::fp(&s.to),
            s.missing.len(),
            job.total,
            s.missing.len() - job.total
        );
        self.holdings_send_missing(s.nonce, s.to, s.addr, &s.missing);
        if !job.pending.is_empty() {
            self.holdings_pulls.push(job);
        }
    }

    /// SIBLING: a fetched blob is stored (`landed`) or its fetch was let go — settle it in every pull job, re-answer the leaver now and then (about thirty-two answers per pull, plus the last), and retire finished jobs.
    pub(super) fn holdings_pull_settled(&mut self, hash: &[u8; 32], landed: bool) {
        self.attach_fetch_prefer.remove(hash);
        let mut answers: Vec<([u8; 32], [u8; 32], std::net::SocketAddr, Vec<[u8; 32]>)> = Vec::new();
        for job in self.holdings_pulls.iter_mut() {
            if !job.pending.remove(hash) {
                continue;
            }
            if landed {
                job.missing.remove(hash);
                job.landed += 1;
            }
            let step = (job.total / 32).max(1);
            if job.pending.is_empty() || (landed && job.landed % step == 0) {
                answers.push((job.nonce, job.leaver, job.addr, job.missing.iter().copied().collect()));
            }
        }
        self.holdings_pulls.retain(|j| !j.pending.is_empty());
        for (nonce, to, addr, missing) in answers {
            self.holdings_send_missing(nonce, to, addr, &missing);
        }
    }

    /// LEAVER, on a `holdings_missing`: file it under the sibling that sent it, if it answers OUR current ask; then a pending copy-then-release moves on (pull, or release). True when it landed.
    pub(super) fn holdings_answer(&mut self, nonce: [u8; 32], sealed: Vec<u8>, from: [u8; 32]) -> bool {
        let key = self.fleet_key_ram_only();
        let Some(check) = self.holdings_check.as_mut() else { return false };
        if check.nonce != nonce || !check.asked.contains(&from) {
            return false;
        }
        let Some(missing) = key.and_then(|k| kete::decrypt_bytes(&sealed, &holdings_key(&k)).ok()).and_then(|p| split_hashes(&p)) else {
            crate::logf!("HOLDINGS: answer from {} did not open — dropped", crate::fp(&from));
            return false;
        };
        crate::logf!("HOLDINGS: {} lacks {} of ours", crate::fp(&from), missing.len());
        let pulled = check.pull.as_ref().filter(|(t, _)| *t == from).map(|(_, set)| set);
        merge_answer(&mut check.answers, from, pulled, missing);
        if self.release_when_copied {
            if !self.holdings_release_if_safe() && self.holdings_check.as_ref().is_some_and(|c| c.pull.is_none()) {
                self.holdings_pull_start();
            }
        }
        true
    }

    /// LEAVER: ask the most complete sibling to fetch the at-risk set from us now. False when there is nothing to pull or nobody to pull it.
    fn holdings_pull_start(&mut self) -> bool {
        let key = self.fleet_key_ram_only();
        let me = self.device_keypair.as_ref().map(|kp| (kp.public.to_bytes(), *kp.secret.as_bytes()));
        let Some(check) = self.holdings_check.as_mut() else { return false };
        let (Some(held), Some(target)) = (check.held.as_ref(), pull_target(&check.answers)) else { return false };
        let lacks = &check.answers[&target];
        let set: HashSet<[u8; 32]> = at_risk(&check.items, held, &check.answers)
            .iter()
            .flat_map(|i| i.hashes.iter().copied())
            .filter(|h| held.contains(h) && lacks.contains(h))
            .collect();
        if set.is_empty() {
            return false;
        }
        let (Some(key), Some((pk, sk))) = (key, me) else { return false };
        let mut list: Vec<[u8; 32]> = set.iter().copied().collect();
        list.sort_unstable();
        let frame = kete::encrypt_bytes(&list.concat(), &holdings_key(&key))
            .and_then(|sealed| crate::network::fgtw::protocol::build_holdings_vsf(HoldingsKind::Pull, &check.nonce, sealed, &pk, &sk));
        match frame {
            Ok(frame) => {
                crate::logf!("HOLDINGS: asked {} to take {} hashes that exist only here", crate::fp(&target), list.len());
                check.pull = Some((target, set));
                self.dispatch_frame_to_sibling(frame, Some(target));
                true
            }
            Err(e) => {
                crate::logf!("HOLDINGS: pull build failed: {}", e);
                false
            }
        }
    }

    /// LEAVER: with copy-then-release pending and nothing at risk any more, clear the flag and send the departure request. True when it released.
    fn holdings_release_if_safe(&mut self) -> bool {
        if !self.release_when_copied || self.depart_request_t.is_some() {
            return false;
        }
        let safe = view(self.holdings_check.as_ref(), &self.contacts).is_some_and(|s| matches!(s.view, HoldingsView::Safe));
        if safe {
            crate::log("HOLDINGS: every item has a second home — sending the departure request");
            self.set_release_when_copied(false);
            self.request_fleet_departure(1);
        }
        safe
    }

    /// The Release card's slot-5 pill: start copy-then-release, or stop waiting.
    pub(super) fn holdings_copy_then_release_tap(&mut self) {
        if self.release_when_copied {
            crate::log("HOLDINGS: copy-then-release cancelled — this device stays");
            self.set_release_when_copied(false);
            if let Some(c) = self.holdings_check.as_mut() {
                c.pull = None;
            }
            return;
        }
        self.set_release_when_copied(true);
        if !self.holdings_pull_start() {
            self.holdings_release_if_safe();
        }
    }

    /// Persist the copy-then-release flag (device-local: it is THIS device that waits to leave).
    pub(super) fn set_release_when_copied(&mut self, on: bool) {
        self.release_when_copied = on;
        if !self.ensure_fleet_settings() {
            return;
        }
        let now = vsf::eagle_time_oscillations();
        let fs = self.fleet_settings.as_mut().unwrap();
        if fs.linked(RELEASE_WHEN_COPIED) {
            fs.set_link(RELEASE_WHEN_COPIED, false, now);
        }
        if fs.set(RELEASE_WHEN_COPIED, vsf::VsfType::u0(on), now) {
            self.persist_and_push_settings();
        }
    }

    /// The flag as stored (absent = off), read where settings apply.
    pub(super) fn load_release_when_copied(&mut self) {
        self.release_when_copied = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.device_local(RELEASE_WHEN_COPIED))
            .and_then(crate::storage::fleet_settings::as_bool)
            .unwrap_or(false);
    }
}

/// The Release card's verdict for the page. `None` = no check.
pub(super) fn view(check: Option<&HoldingsCheck>, contacts: &[crate::types::Contact]) -> Option<HoldingsShown> {
    let check = check?;
    let shown = |view, waiting| HoldingsShown {
        view,
        waiting,
        target: check.pull.as_ref().map(|(t, _)| *t).or_else(|| pull_target(&check.answers)).map(|t| sibling_name(contacts, &t)),
    };
    if check.no_key {
        return Some(shown(HoldingsView::NoKey, Vec::new()));
    }
    let Some(held) = check.held.as_ref() else {
        return Some(shown(HoldingsView::Checking, Vec::new()));
    };
    let waiting: Vec<String> = check
        .asked
        .iter()
        .filter(|d| !check.answers.contains_key(*d))
        .map(|d| {
            let heard = match contacts.iter().find(|c| c.is_sibling && c.knows_device(d)).and_then(|c| c.last_heard) {
                Some(at) => ago_label(at.elapsed().as_secs() as i64 * crate::OSC_PER_SEC),
                None => tr(Msg::HoldingsNotHeard).into_owned(),
            };
            format!("{} ({heard})", sibling_name(contacts, d))
        })
        .collect();
    // Nobody has answered yet while somebody was asked: no verdict to give.
    if check.answers.is_empty() && !check.asked.is_empty() && !held.is_empty() {
        return Some(shown(HoldingsView::Checking, waiting));
    }
    let risk = at_risk(&check.items, held, &check.answers);
    if risk.is_empty() {
        return Some(shown(HoldingsView::Safe, waiting));
    }
    let recordings = risk.iter().filter(|i| i.recording).count() as u64;
    let bytes = risk.iter().map(|i| i.size).fold(0u64, u64::saturating_add);
    Some(shown(HoldingsView::AtRisk { files: risk.len() as u64 - recordings, recordings, bytes }, waiting))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(hashes: &[u8]) -> HeldItem {
        HeldItem { recording: false, size: 1, hashes: hashes.iter().map(|b| [*b; 32]).collect() }
    }

    fn set(bytes: &[u8]) -> HashSet<[u8; 32]> {
        bytes.iter().map(|b| [*b; 32]).collect()
    }

    /// Per ITEM, not per device: an item is safe once ANY answering sibling holds every hash of it we hold; a partial holder vouches only for what it has.
    #[test]
    fn at_risk_counts_holders_per_item() {
        let items = vec![item(&[1]), item(&[2, 3]), item(&[4])];
        let held = set(&[1, 2, 3, 4]);
        let watch = set(&[2, 3, 4]); // lacks all but item 1
        let laptop = set(&[1, 3]); // lacks item 1 and the head of item 2
        let answers: HashMap<[u8; 32], HashSet<[u8; 32]>> = [([10u8; 32], watch), ([11u8; 32], laptop)].into_iter().collect();
        let risk = at_risk(&items, &held, &answers);
        // Item 1 is on the watch and item 4 on the laptop; item 2's head (3) is on neither, so item 2 is at risk although the laptop holds its original.
        assert_eq!(risk.len(), 1);
        assert_eq!(risk[0].hashes[0], [2u8; 32]);
        // With nobody answering, everything held is at risk; a hash we do not hold never puts an item at risk.
        assert_eq!(at_risk(&items, &held, &HashMap::new()).len(), 3);
        assert_eq!(at_risk(&items, &set(&[1]), &HashMap::new()).len(), 1);
    }

    /// The copy goes to the most complete holder; its answers to the pull replace only the pulled part of what it lacked, so a hash it still lacks outside the pull keeps counting against it.
    #[test]
    fn pull_answers_merge_into_the_pulled_part_only() {
        let mut answers: HashMap<[u8; 32], HashSet<[u8; 32]>> = [([10u8; 32], set(&[1, 2, 3])), ([11u8; 32], set(&[1, 2, 5, 6]))].into_iter().collect();
        assert_eq!(pull_target(&answers), Some([10u8; 32]));
        let pulled = set(&[1, 2]);
        merge_answer(&mut answers, [10u8; 32], Some(&pulled), vec![[2u8; 32]]);
        assert_eq!(answers[&[10u8; 32]], set(&[2, 3]));
        merge_answer(&mut answers, [10u8; 32], Some(&pulled), vec![]);
        assert_eq!(answers[&[10u8; 32]], set(&[3]));
        // An answer to the plain ask replaces the whole entry.
        merge_answer(&mut answers, [11u8; 32], None, vec![[7u8; 32]]);
        assert_eq!(answers[&[11u8; 32]], set(&[7]));
    }

    #[test]
    fn split_hashes_rejects_ragged_lists() {
        assert_eq!(split_hashes(&[]), Some(vec![]));
        assert_eq!(split_hashes(&[7u8; 64]), Some(vec![[7u8; 32], [7u8; 32]]));
        assert_eq!(split_hashes(&[7u8; 33]), None);
    }
}
