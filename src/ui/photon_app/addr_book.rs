//! A person's devices hand their address book to an alias sibling (network/addr_book.rs), so a Photon that cannot ask fgtw.org (ferros's) still reaches friends and siblings: the owner 2026-11-18, "the bootstrap is literally supposed to be a fallback".
//! Sent sealed under the pair key our sibling ceremony made with that one device, on the edges that change what it should know: its pair secret first read here, it coming online, an address of ours or a friend's changing.
//! Taken by any device: each signed record verifies by the device it describes and must belong to someone we hold (a sibling of ours, a friend's fold, a group member's fold), newest wins in the phonebook, and what it adds goes to the punch the way a record from fgtw.org does.

use super::*;
use crate::network::addr_book::{AddrBook, HeardAddr};
use crate::network::fgtw::{PeerRecord, PeerStore};
use crate::types::contact::AddrSource;
use crate::types::molecule::MoleculePeer;

impl PhotonApp {
    /// The key our address book travels under between this device and sibling `device`: the pair key for an alias sibling (alias_seal_key); None for a fleet sibling, which gossips phonebooks with us already.
    fn addr_book_key(&self, device: &[u8; 32]) -> Option<[u8; 32]> {
        self.alias_seal_key(device, b"addr_book")
    }

    /// What we hold that sibling `to` may not: our own record signed now, every signed record of a device we can bind to someone we hold, and every address we heard a known device at ourselves.
    fn addr_book_for(&self, to: &[u8; 32]) -> AddrBook {
        let (Some(own_hp), Some(own_dev)) = (self.our_handle_proof(), self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes())) else {
            return AddrBook::default();
        };
        let held = self.peer_store.as_ref().map(|s| s.lock().unwrap().get_all_peers()).unwrap_or_default();
        let mut book = book_from(&held, &self.contacts, &self.molecule_peers, own_hp, own_dev, to);
        if let Some(own) = self.own_peer_record() {
            book.records.insert(0, own);
        }
        book
    }

    /// Hand our address book to each alias sibling, or `only` that one, sealed for it alone. No fleet sibling gets it (they gossip phonebooks already) and nothing goes to a sibling whose pair secret is not read here yet: the read's own edge (drain_alias_pairs) sends it then.
    pub(super) fn addr_book_push(&self, only: Option<[u8; 32]>, why: &str) {
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        let sibs: Vec<([u8; 32], [u8; 32])> = self
            .contacts
            .iter()
            .filter(|c| c.is_sibling && !c.locked_out)
            .filter_map(|c| c.device_key())
            .filter(|d| only.is_none_or(|o| o == *d))
            .filter_map(|d| self.addr_book_key(&d).map(|k| (d, k)))
            .collect();
        for (dev, key) in sibs {
            let book = self.addr_book_for(&dev);
            if book.records.is_empty() && book.heard.is_empty() {
                continue;
            }
            let frame = crate::network::addr_book::seal(&book, &key).and_then(|s| crate::network::fgtw::protocol::build_addr_book_vsf(s, kp.public.as_bytes(), kp.secret.as_bytes()));
            match frame {
                Ok(f) => {
                    crate::logf!("ADDRBOOK: {} signed record(s) and {} heard address(es) to alias sibling {} ({})", book.records.len(), book.heard.len(), crate::fp(&dev), why);
                    self.dispatch_frame_to_sibling(f, Some(dev));
                }
                Err(e) => crate::logf!("ADDRBOOK: the book for sibling {} did not build ({})", crate::fp(&dev), e),
            }
        }
    }

    /// A sibling's address book: opened under the key we share with it alone, each record checked and adopted newest-wins, and whatever it taught goes to the punch at once. True when it changed anything.
    pub(super) fn take_addr_book(&mut self, sealed: &[u8], sender: [u8; 32]) -> bool {
        let Some(key) = self.addr_book_key(&sender) else {
            crate::logf!("ADDRBOOK: a book from sibling {} before its pair secret is read here — dropped; its next push lands", crate::fp(&sender));
            return false;
        };
        let book = match crate::network::addr_book::open(sealed, &key) {
            Ok(b) => b,
            Err(e) => {
                crate::logf!("ADDRBOOK: the book from sibling {} did not open ({}) — dropped", crate::fp(&sender), e);
                return false;
            }
        };
        let (Some(store), Some(own_hp), Some(own_dev)) = (self.peer_store.clone(), self.our_handle_proof(), self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes())) else {
            return false;
        };
        let got = {
            let mut s = store.lock().unwrap();
            adopt_book(book, &mut s, &mut self.contacts, &self.molecule_peers, own_hp, own_dev)
        };
        crate::logf!(
            "ADDRBOOK: from sibling {}: {} record(s) newer than held, {} endpoint(s) learned, {} heard address(es) to probe, {} refused{}",
            crate::fp(&sender),
            got.merged,
            got.endpoints,
            got.hinted,
            got.refused.len(),
            if got.refused.is_empty() { String::new() } else { format!(" ({})", got.refused.join(", ")) }
        );
        if got.merged > 0 {
            self.request_peer_persist();
        }
        if got.endpoints + got.hinted == 0 {
            return got.merged > 0;
        }
        // The same edge a record from fgtw.org raises: punch now, and every fetch that found nobody to ask asks again.
        self.stalled_refetch_streak = 0;
        self.ping_contacts();
        self.attach_address_edge("a sibling's address book");
        true
    }
}

/// What a book adoption did.
#[derive(Debug, Default)]
pub(super) struct Adopted {
    /// Records the phonebook took as newer than what it held.
    pub merged: usize,
    /// Contact endpoints a newer record changed.
    pub endpoints: usize,
    /// Heard addresses noted as somewhere to probe.
    pub hinted: usize,
    /// Why each refused record was refused (device fingerprint and reason).
    pub refused: Vec<String>,
}

/// Whose is device `dev`, claimed under `hp`, by what this device holds? Ok carries the fold the phonebook merge is gated on (None when no fold binds it here); Err says why it belongs to nobody we hold.
/// `strict` (the sender's side) asks that a friend's device be one we know; the receiver takes a friend's device its sibling vouched for unless a fold held here excludes it, since our own device's word is ours, and the punch still has to prove the address.
fn bind_device(contacts: &[crate::types::Contact], peers: &[MoleculePeer], own_hp: &[u8; 32], hp: &[u8; 32], dev: &[u8; 32], strict: bool) -> Result<Option<Vec<[u8; 32]>>, &'static str> {
    if hp == own_hp {
        return match contacts.iter().find(|c| c.is_sibling && c.device_key() == Some(*dev)) {
            Some(c) if c.locked_out => Err("a locked-out device of ours"),
            Some(_) => Ok(None),
            None => Err("not a device of ours"),
        };
    }
    if let Some(c) = contacts.iter().find(|c| !c.is_sibling && c.handle_proof == *hp) {
        if c.refused_devices.contains(dev) {
            return Err("refused by its owner's fleet");
        }
        if c.fleet_folded_once {
            return if c.fleet_members.contains(dev) { Ok(Some(c.fleet_members.clone())) } else { Err("outside the friend's fold") };
        }
        if strict && !c.knows_device(dev) {
            return Err("a device the friend never showed us");
        }
        return Ok(None);
    }
    match peers.iter().find(|p| p.handle_proof == *hp) {
        Some(p) if p.knows_device(dev) => Ok(Some(p.devices.clone())),
        Some(_) => Err("outside the group member's fold"),
        None => Err("nobody we hold"),
    }
}

/// The book this device hands sibling `to`: every held record that verifies and binds to someone we hold, and every other address a known device is held at here (heard, beaconed, or adopted from a seed record whose signature was not kept). Nothing about `to` itself or about us (our own record is signed fresh by the caller), and no hint passed on from another sibling.
pub(super) fn book_from(held: &[PeerRecord], contacts: &[crate::types::Contact], peers: &[MoleculePeer], own_hp: [u8; 32], own_dev: [u8; 32], to: &[u8; 32]) -> AddrBook {
    let skip = |d: &[u8; 32]| *d == own_dev || d == to;
    let records: Vec<PeerRecord> = held
        .iter()
        .filter(|r| !skip(r.device_pubkey.as_bytes()) && r.verify() && bind_device(contacts, peers, &own_hp, &r.handle_proof, r.device_pubkey.as_bytes(), true).is_ok())
        .cloned()
        .collect();
    // An address a signed record above already carries goes only there.
    let signed = |dev: &[u8; 32], addr: std::net::SocketAddr| records.iter().any(|r| r.device_pubkey.as_bytes() == dev && (r.ip == addr || r.lan_addr() == Some(addr)));
    let mut heard = Vec::new();
    for c in contacts.iter().filter(|c| !c.locked_out) {
        let hp = if c.is_sibling { own_hp } else { c.handle_proof };
        for ep in c.device_endpoints.iter().filter(|ep| !skip(&ep.pubkey) && c.knows_device(&ep.pubkey)) {
            for a in ep.addrs.iter().filter(|a| a.how != AddrSource::Hint && !signed(&ep.pubkey, a.addr)) {
                let h = HeardAddr { handle_proof: hp, device: ep.pubkey, addr: a.addr, heard: a.heard };
                if !heard.contains(&h) {
                    heard.push(h);
                }
            }
        }
    }
    AddrBook { records, heard }
}

/// Take a sibling's book into `store` and `contacts`: a record whose signature fails, or whose device belongs to nobody we hold, is refused; the rest merge newest-wins (merge_peer_bound, gated on the fold we hold), and only a record the phonebook took as newer moves an endpoint.
/// A heard address lands only on a device we know, as a hint behind everything heard or signed here.
pub(super) fn adopt_book(book: AddrBook, store: &mut PeerStore, contacts: &mut [crate::types::Contact], peers: &[MoleculePeer], own_hp: [u8; 32], own_dev: [u8; 32]) -> Adopted {
    let mut got = Adopted::default();
    let locked: Vec<[u8; 32]> = contacts.iter().filter(|c| c.is_sibling && c.locked_out).filter_map(|c| c.device_key()).collect();
    for rec in book.records {
        let dev = *rec.device_pubkey.as_bytes();
        if dev == own_dev {
            continue;
        }
        if !rec.verify() {
            got.refused.push(format!("{}: signature", crate::fp(&dev)));
            continue;
        }
        let fold = match bind_device(contacts, peers, &own_hp, &rec.handle_proof, &dev, false) {
            Ok(f) => f,
            Err(why) => {
                got.refused.push(format!("{}: {}", crate::fp(&dev), why));
                continue;
            }
        };
        let (hp, ip, lan) = (rec.handle_proof, rec.ip, rec.lan_addr());
        if !store.merge_peer_bound(rec, fold.as_deref()) {
            continue;
        }
        got.merged += 1;
        // The record the phonebook now holds for the device, at the stamp it holds it under.
        let seen = store.get_devices_for_handle(&hp).into_iter().find(|r| r.device_pubkey.as_bytes() == &dev).map_or(0, |r| r.local_seen);
        got.endpoints += super::peers::adopt_endpoint_into(contacts, &locked, hp, dev, ip, lan.as_slice(), seen, true);
    }
    for h in book.heard {
        if h.device == own_dev || crate::network::traverse::gather::is_bogus_addr(&h.addr) || bind_device(contacts, peers, &own_hp, &h.handle_proof, &h.device, true).is_err() {
            continue;
        }
        for c in contacts.iter_mut().filter(|c| if h.handle_proof == own_hp { c.is_sibling && c.device_key() == Some(h.device) } else { !c.is_sibling && c.handle_proof == h.handle_proof }) {
            if c.endpoint_mut(&h.device).hint(h.addr, h.heard) {
                got.hinted += 1;
            }
        }
    }
    got
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Contact, DevicePubkey, HandleText};
    use ed25519_dalek::SigningKey;

    const OWN_HP: [u8; 32] = [0xA0; 32];
    const OWN_DEV: [u8; 32] = [0xA1; 32];

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn pubkey(seed: u8) -> [u8; 32] {
        key(seed).verifying_key().to_bytes()
    }

    fn record(seed: u8, hp: [u8; 32], ip: &str, at: i64) -> PeerRecord {
        let mut r = PeerRecord::new(hp, DevicePubkey::from_bytes(pubkey(seed)), ip.parse().unwrap());
        r.last_seen = at;
        r.local_ip = None;
        r.sign(&key(seed));
        r
    }

    fn friend(hp: [u8; 32], dev: [u8; 32]) -> Contact {
        Contact::new(HandleText::new("friend"), hp, DevicePubkey::from_bytes(dev))
    }

    fn sibling(dev: [u8; 32]) -> Contact {
        let mut c = Contact::new(HandleText::new("sibling"), OWN_HP, DevicePubkey::from_bytes(dev));
        c.is_sibling = true;
        c
    }

    fn now() -> i64 {
        crate::network::time_base::now_osc()
    }

    /// A fleet device's book round-trips to an alias sibling: our sibling's and a friend's signed records come out of the seal still verifying, land in the receiver's phonebook and on the right contacts' endpoints, and a heard address lands as a hint to probe.
    #[test]
    fn a_fleet_devices_book_reaches_an_alias_sibling_and_verifies() {
        let friend_hp = [0x22; 32];
        let (desktop, laptop, phone) = (pubkey(1), pubkey(2), pubkey(3));
        // The sender (the desktop) holds its laptop and a friend's phone.
        let mut sender_contacts = vec![sibling(laptop), sibling(OWN_DEV), friend(friend_hp, phone)];
        sender_contacts[2].endpoint_mut(&phone).note("198.51.100.4:51000".parse().unwrap(), AddrSource::Heard);
        let held = vec![record(2, OWN_HP, "203.0.113.9:4383", now() - 10), record(3, friend_hp, "203.0.113.20:4383", now() - 5)];
        let book = book_from(&held, &sender_contacts, &[], OWN_HP, desktop, &OWN_DEV);
        assert_eq!((book.records.len(), book.heard.len()), (2, 1));
        let k = [0x42; 32];
        let opened = crate::network::addr_book::open(&crate::network::addr_book::seal(&book, &k).unwrap(), &k).unwrap();
        assert!(opened.records.iter().all(|r| r.verify()), "every handed-over record verifies on arrival");
        // The receiver (the alias sibling) knows the desktop, the laptop and the friend's phone.
        let mut contacts = vec![sibling(desktop), sibling(laptop), friend(friend_hp, phone)];
        let mut store = PeerStore::new();
        let got = adopt_book(opened, &mut store, &mut contacts, &[], OWN_HP, OWN_DEV);
        assert_eq!((got.merged, got.refused.len()), (2, 0), "{:?}", got.refused);
        assert_eq!(store.get_all_peers().len(), 2, "both records are in the receiver's phonebook");
        assert!(contacts[1].device_endpoints.iter().any(|e| e.pubkey == laptop && e.has("203.0.113.9:4383".parse().unwrap())), "the sibling's address reached its row");
        let ep = contacts[2].device_endpoints.iter().find(|e| e.pubkey == phone).unwrap();
        assert!(ep.has("203.0.113.20:4383".parse().unwrap()), "the friend's signed address reached the friend's row");
        assert!(ep.addrs.iter().any(|a| a.how == AddrSource::Hint && a.addr == "198.51.100.4:51000".parse().unwrap()), "the heard address is a hint to probe");
    }

    /// A forged record (its address changed after signing), a record signed by another key than the device it names, a device outside the friend's fold and a device of nobody we hold are each refused; nothing of them reaches the phonebook or a contact.
    #[test]
    fn a_forged_or_mis_signed_record_is_refused() {
        let friend_hp = [0x22; 32];
        let (phone, other) = (pubkey(3), pubkey(4));
        let mut forged = record(3, friend_hp, "203.0.113.20:4383", now() - 5);
        forged.ip = "6.6.6.6:4383".parse().unwrap();
        let mut mis_signed = PeerRecord::new(friend_hp, DevicePubkey::from_bytes(phone), "7.7.7.7:4383".parse().unwrap());
        mis_signed.sign(&key(9));
        let outside = record(4, friend_hp, "8.8.8.8:4383", now() - 5);
        let nobody = record(5, [0x77; 32], "9.9.9.9:4383", now() - 5);
        let mut f = friend(friend_hp, phone);
        f.fleet_members = vec![phone];
        f.fleet_folded_once = true;
        let mut contacts = vec![f];
        let mut store = PeerStore::new();
        let book = AddrBook { records: vec![forged, mis_signed, outside, nobody], heard: vec![HeardAddr { handle_proof: friend_hp, device: other, addr: "1.2.3.4:4383".parse().unwrap(), heard: 1 }] };
        let got = adopt_book(book, &mut store, &mut contacts, &[], OWN_HP, OWN_DEV);
        assert_eq!(got.refused.len(), 4, "{:?}", got.refused);
        assert_eq!((got.merged, got.endpoints, got.hinted), (0, 0, 0), "a hint about a device outside the fold is dropped too");
        assert!(store.get_all_peers().is_empty());
        assert!(contacts[0].device_endpoints.is_empty());
    }

    /// Newest wins: a newer record moves the endpoint, an older one handed over after it changes nothing in the phonebook or on the contact.
    #[test]
    fn the_receiver_adopts_newest_wins() {
        let friend_hp = [0x22; 32];
        let phone = pubkey(3);
        let mut contacts = vec![friend(friend_hp, phone)];
        let mut store = PeerStore::new();
        // Sixty-four seconds apart, far wider than the time between the two adoptions.
        let t = now();
        let newer = AddrBook { records: vec![record(3, friend_hp, "203.0.113.20:4383", t)], heard: vec![] };
        assert_eq!(adopt_book(newer, &mut store, &mut contacts, &[], OWN_HP, OWN_DEV).merged, 1);
        let older = AddrBook { records: vec![record(3, friend_hp, "203.0.113.99:4383", t - crate::OSC_PER_SEC * (1 << 6))], heard: vec![] };
        let got = adopt_book(older, &mut store, &mut contacts, &[], OWN_HP, OWN_DEV);
        assert_eq!((got.merged, got.endpoints), (0, 0), "an older record is not adopted");
        assert_eq!(store.get_all_peers()[0].ip, "203.0.113.20:4383".parse().unwrap(), "the phonebook keeps the newer record");
        let ep = contacts[0].device_endpoints.iter().find(|e| e.pubkey == phone).unwrap();
        assert!(!ep.has("203.0.113.99:4383".parse().unwrap()), "the stale address never reaches the punch");
    }

    /// The sender hands over only what binds: a friend's device it never saw is left out, as is anything about the sibling it is talking to and its own row.
    #[test]
    fn the_sender_hands_over_only_what_binds() {
        let friend_hp = [0x22; 32];
        let phone = pubkey(3);
        let contacts = vec![friend(friend_hp, phone), sibling(OWN_DEV)];
        let held = vec![record(3, friend_hp, "203.0.113.20:4383", now()), record(4, friend_hp, "203.0.113.21:4383", now())];
        let book = book_from(&held, &contacts, &[], OWN_HP, pubkey(1), &OWN_DEV);
        assert_eq!(book.records.len(), 1);
        assert_eq!(book.records[0].device_pubkey.as_bytes(), &phone);
    }
}
