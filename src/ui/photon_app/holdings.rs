//! DEVICE-REMOVAL SAFETY, phase 2 (Nick 2026-10-17: "is there a way to let the user know they're removing a device that has the only copy and to wait for sync or offer other options?").
//!
//! Arming Release starts a have-set exchange: this device lists the attachment originals (and their heads) it actually holds, seals the list under the fleet key, and asks every live sibling which of them it LACKS.
//! An item is at risk when some hash of it that we hold is missing on every sibling that answered — holders are counted per item, never per device, so a watch that holds three photos vouches for those three and nothing else.
//! The Security page shows the result under the Release pill (checking, all safe, or what exists only here) plus the siblings that have not answered and when we last heard from them; the second tap still releases.
//! The list carries RAW content hashes: presence follows the raw→hb alias, so a wrapped original answers the same as a raw one; the ask moves to header hb when the raw form retires at v134.
//! Every presence probe runs on a worker (the UI thread makes no vault call); the fleet key is read from RAM only.

use super::*;
use std::collections::{HashMap, HashSet};

/// One attachment this device's rows point at: the original first, then its head when it has one.
pub(super) struct HeldItem {
    recording: bool,
    size: u64,
    hashes: Vec<[u8; 32]>,
}

/// The leaver's check in flight (or landed) — one per arm of the Release pill.
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
}

/// A sibling's finished answer, handed from its presence worker back to the UI thread to seal, sign and send.
pub(super) struct HoldingsServe {
    nonce: [u8; 32],
    to: [u8; 32],
    addr: std::net::SocketAddr,
    missing: Vec<[u8; 32]>,
}

/// What the Security page draws under the Release pill.
pub(super) enum HoldingsView {
    Checking,
    NoKey,
    Safe,
    AtRisk { files: u64, recordings: u64, bytes: u64 },
}

/// The seal key for both frames: a domain-separated derivation of the fleet key, so a holdings list never shares a key with any other fleet ciphertext.
fn holdings_key(fleet_key: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"PHOTON_FLEET_HOLDINGS_v\x01");
    h.update(fleet_key);
    *h.finalize().as_bytes()
}

/// A sealed list's plaintext as hashes; a length that is not a whole number of hashes is a malformed list and reads as none.
fn split_hashes(plain: &[u8]) -> Option<Vec<[u8; 32]>> {
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

impl PhotonApp {
    fn fleet_key_ram_only(&self) -> Option<[u8; 32]> {
        self.fleet_key_ram.lock().ok().and_then(|g| *g)
    }

    /// LEAVER, on arming Release: list every attachment the rows point at and hand the presence probe to a worker; the ask goes out when it lands ([`Self::drain_holdings`]).
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
        });
    }

    /// Both halves' worker results: the leaver's probe landing (send the ask), and a sibling's finished answers (send each). True when the page should repaint.
    pub(super) fn drain_holdings(&mut self) -> bool {
        let mut changed = false;
        let landed = self.holdings_check.as_ref().and_then(|c| c.probe_rx.as_ref()).and_then(|rx| rx.try_recv().ok());
        if let Some(held) = landed {
            self.holdings_send_ask(held);
            changed = true;
        }
        let served: Vec<HoldingsServe> = self.holdings_serve_rx.try_iter().collect();
        for s in served {
            self.holdings_send_missing(s);
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
            return;
        }
        let (Some(key), Some((pk, sk))) = (key, me) else {
            crate::log("HOLDINGS: no fleet key in RAM — cannot seal the ask");
            check.no_key = true;
            return;
        };
        let frame = kete::encrypt_bytes(&list.concat(), &holdings_key(&key))
            .and_then(|sealed| crate::network::fgtw::protocol::build_holdings_vsf(true, &check.nonce, sealed, &pk, &sk));
        match frame {
            Ok(frame) => {
                crate::logf!("HOLDINGS: asked {} siblings about {} held hashes", siblings.len(), list.len());
                check.asked = siblings;
                self.dispatch_frame_to_siblings(frame);
            }
            Err(e) => {
                crate::logf!("HOLDINGS: ask build failed: {}", e);
                check.no_key = true;
            }
        }
    }

    /// SIBLING, on a `holdings_ask` (sender already gated as a live member): open the list and probe it on a worker; the answer goes out from [`Self::drain_holdings`].
    pub(super) fn holdings_serve(&mut self, nonce: [u8; 32], sealed: Vec<u8>, from: [u8; 32], addr: std::net::SocketAddr) {
        let Some(key) = self.fleet_key_ram_only() else {
            crate::log("HOLDINGS: ask from a sibling but no fleet key in RAM — unanswered");
            return;
        };
        let Some(asked) = kete::decrypt_bytes(&sealed, &holdings_key(&key)).ok().and_then(|p| split_hashes(&p)) else {
            crate::logf!("HOLDINGS: ask from {} did not open — dropped", crate::fp(&from));
            return;
        };
        let tx = self.holdings_serve_tx.clone();
        let wake = self.event_proxy.clone();
        let _ = std::thread::Builder::new().name("holdings-serve".into()).spawn(move || {
            let missing: Vec<[u8; 32]> = asked.into_iter().filter(|h| !crate::storage::blob_present_probe_now(h)).collect();
            let _ = tx.send(HoldingsServe { nonce, to: from, addr, missing });
            if let Some(w) = wake {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });
    }

    fn holdings_send_missing(&mut self, s: HoldingsServe) {
        let (Some(key), Some(kp), Some(checker)) = (self.fleet_key_ram_only(), self.device_keypair.as_ref(), self.status_checker.as_ref()) else {
            return;
        };
        let frame = kete::encrypt_bytes(&s.missing.concat(), &holdings_key(&key)).and_then(|sealed| {
            crate::network::fgtw::protocol::build_holdings_vsf(false, &s.nonce, sealed, &kp.public.to_bytes(), kp.secret.as_bytes())
        });
        match frame {
            Ok(frame) => {
                crate::logf!("HOLDINGS: answered {} — missing {}", crate::fp(&s.to), s.missing.len());
                let _ = checker.history_dispatch().send(crate::network::status::HistorySendRequest {
                    peer_addr: s.addr,
                    alt_addr: None,
                    recipient_pubkey: s.to,
                    relay_to: vec![s.to],
                    vsf_bytes: frame,
                    tag: None,
                });
            }
            Err(e) => crate::logf!("HOLDINGS: answer build failed: {}", e),
        }
    }

    /// LEAVER, on a `holdings_missing`: file it under the sibling that sent it, if it answers OUR current ask. True when it landed.
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
        check.answers.insert(from, missing.into_iter().collect());
        true
    }
}

/// The Release card's verdict, plus a label per asked sibling that has not answered yet (name and when we last heard from it). `None` = no check.
pub(super) fn view(check: Option<&HoldingsCheck>, contacts: &[crate::types::Contact]) -> Option<(HoldingsView, Vec<String>)> {
    let check = check?;
    if check.no_key {
        return Some((HoldingsView::NoKey, Vec::new()));
    }
    let Some(held) = check.held.as_ref() else {
        return Some((HoldingsView::Checking, Vec::new()));
    };
    let waiting: Vec<String> = check
        .asked
        .iter()
        .filter(|d| !check.answers.contains_key(*d))
        .map(|d| {
            let row = contacts.iter().find(|c| c.is_sibling && c.knows_device(d));
            let name = row.map(|c| c.display_name()).unwrap_or_else(|| crate::fp(d));
            let heard = match row.and_then(|c| c.last_heard) {
                Some(at) => ago_label(at.elapsed().as_secs() as i64 * crate::OSC_PER_SEC),
                None => tr(Msg::HoldingsNotHeard).into_owned(),
            };
            format!("{name} ({heard})")
        })
        .collect();
    // Nobody has answered yet while somebody was asked: no verdict to give.
    if check.answers.is_empty() && !check.asked.is_empty() && !held.is_empty() {
        return Some((HoldingsView::Checking, waiting));
    }
    let risk = at_risk(&check.items, held, &check.answers);
    if risk.is_empty() {
        return Some((HoldingsView::Safe, waiting));
    }
    let recordings = risk.iter().filter(|i| i.recording).count() as u64;
    let bytes = risk.iter().map(|i| i.size).fold(0u64, u64::saturating_add);
    Some((HoldingsView::AtRisk { files: risk.len() as u64 - recordings, recordings, bytes }, waiting))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(hashes: &[u8]) -> HeldItem {
        HeldItem { recording: false, size: 1, hashes: hashes.iter().map(|b| [*b; 32]).collect() }
    }

    /// Per ITEM, not per device: an item is safe once ANY answering sibling holds every hash of it we hold; a partial holder vouches only for what it has.
    #[test]
    fn at_risk_counts_holders_per_item() {
        let items = vec![item(&[1]), item(&[2, 3]), item(&[4])];
        let held: HashSet<[u8; 32]> = [[1u8; 32], [2; 32], [3; 32], [4; 32]].into_iter().collect();
        let watch: HashSet<[u8; 32]> = [[2u8; 32], [3; 32], [4; 32]].into_iter().collect(); // lacks all but item 1
        let laptop: HashSet<[u8; 32]> = [[1u8; 32], [3; 32]].into_iter().collect(); // lacks item 1 and the head of item 2
        let answers: HashMap<[u8; 32], HashSet<[u8; 32]>> = [([10u8; 32], watch), ([11u8; 32], laptop)].into_iter().collect();
        let risk = at_risk(&items, &held, &answers);
        // Item 1 is on the watch and item 4 on the laptop; item 2's head (3) is on neither, so item 2 is at risk although the laptop holds its original.
        assert_eq!(risk.len(), 1);
        assert_eq!(risk[0].hashes[0], [2u8; 32]);
        // With nobody answering, everything held is at risk; a hash we do not hold never puts an item at risk.
        assert_eq!(at_risk(&items, &held, &HashMap::new()).len(), 3);
        let held_some: HashSet<[u8; 32]> = [[1u8; 32]].into_iter().collect();
        assert_eq!(at_risk(&items, &held_some, &HashMap::new()).len(), 1);
    }

    #[test]
    fn split_hashes_rejects_ragged_lists() {
        assert_eq!(split_hashes(&[]), Some(vec![]));
        assert_eq!(split_hashes(&[7u8; 64]), Some(vec![[7u8; 32], [7u8; 32]]));
        assert_eq!(split_hashes(&[7u8; 33]), None);
    }
}
