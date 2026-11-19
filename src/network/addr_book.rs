//! THE ADDRESS BOOK one Photon of a person hands another (the owner 2026-11-18: "since I can obviously pull the phonebook from my other devices I shouldn't even need the bootstrap").
//! An alias sibling (Photon on ferros) cannot ask fgtw.org, so a device of ours hands it what it holds: every signed phonebook record of our own devices, our friends' and the group members', and the addresses it merely heard devices at.
//! A signed record verifies by the signature of the device it describes, wherever it lands, exactly as one from fgtw.org would.
//! A heard address carries no signature: the receiver probes it and never takes it as proof.
//! On the wire it is one typed VSF document, sealed for one sibling (ui/photon_app/addr_book.rs) and carried in a device-signed `addr_book` frame (network/fgtw/protocol.rs).

use crate::network::fgtw::PeerRecord;
use std::net::{IpAddr, SocketAddr};
use vsf::VsfType;

/// The document's one section.
const SECTION: &str = "addr_book";

/// An address a device of ours holds for another device with no signature to hand on (it heard the device there, the device beaconed from there, or a phonebook record the device signed was adopted and not kept): the receiver only probes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeardAddr {
    /// The identity the device belongs to, as the sender holds it.
    pub handle_proof: [u8; 32],
    pub device: [u8; 32],
    pub addr: SocketAddr,
    /// When the sender last heard the device there (Eagle oscillations).
    pub heard: i64,
}

/// What one device hands another: signed records and heard addresses.
#[derive(Clone, Debug, Default)]
pub struct AddrBook {
    pub records: Vec<PeerRecord>,
    pub heard: Vec<HeardAddr>,
}

/// The book as one complete VSF document: one `peer` row per signed record (the phonebook gossip's own row, so the signature travels byte for byte) and one `heard` row per heard address.
pub fn encode(book: &AddrBook) -> Result<Vec<u8>, String> {
    let mut section = vsf::VsfSection::new(SECTION);
    for rec in &book.records {
        let (name, values) = crate::network::fgtw::protocol::encode_peer_field(rec);
        section.add_field_multi(name, values);
    }
    for h in &book.heard {
        let ip = match h.addr.ip().to_canonical() {
            IpAddr::V4(v4) => v4.octets().to_vec(),
            IpAddr::V6(v6) => v6.octets().to_vec(),
        };
        section.add_field_multi(
            "heard",
            vec![
                VsfType::hP(h.handle_proof.to_vec()),
                VsfType::ke(h.device.to_vec()),
                VsfType::t_u3(vsf::Tensor::new(vec![ip.len()], ip)),
                VsfType::u4(h.addr.port()),
                VsfType::e(vsf::types::EtType::e6(h.heard)),
            ],
        );
    }
    vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("address book build: {e}"))
}

/// Read a book back: a verified read (hp + hb) before any field is trusted.
/// A row that does not parse is skipped, never the whole book; each record's signature is the receiver's to check (it is not checked here, so a test can see a forged one arrive).
pub fn decode(bytes: &[u8]) -> Result<AddrBook, String> {
    let (header, header_end) = vsf::verification::read_verified(bytes, None).map_err(|e| format!("address book verification: {e}"))?;
    let section = header.primary_section(bytes, header_end).map_err(|e| format!("address book section: {e}"))?;
    if section.name != SECTION {
        return Err(format!("expected an '{SECTION}' section, got '{}'", section.name));
    }
    let records = section.get_fields("peer").into_iter().filter_map(|f| crate::network::fgtw::bootstrap::parse_peer_from_field(f).ok()).collect();
    let heard = section.get_fields("heard").into_iter().filter_map(|f| heard_from_values(&f.values)).collect();
    Ok(AddrBook { records, heard })
}

/// One `heard` row, each value told by its type marker, never by its place.
fn heard_from_values(values: &[VsfType]) -> Option<HeardAddr> {
    let (mut hp, mut device, mut ip, mut port, mut heard) = (None, None, None, None, None);
    for v in values {
        match v {
            VsfType::hP(b) => hp = <[u8; 32]>::try_from(b.as_slice()).ok(),
            VsfType::ke(b) => device = <[u8; 32]>::try_from(b.as_slice()).ok(),
            VsfType::t_u3(t) => {
                ip = match t.data.len() {
                    4 => <[u8; 4]>::try_from(t.data.as_slice()).ok().map(IpAddr::from),
                    16 => <[u8; 16]>::try_from(t.data.as_slice()).ok().map(IpAddr::from),
                    _ => None,
                }
            }
            VsfType::e(vsf::types::EtType::e6(osc)) => heard = Some(*osc),
            other => {
                if let Some(p) = other.as_u64().and_then(|p| u16::try_from(p).ok()) {
                    port = Some(p);
                }
            }
        }
    }
    Some(HeardAddr { handle_proof: hp?, device: device?, addr: SocketAddr::new(ip?.to_canonical(), port?), heard: heard? })
}

/// Seal a book for one sibling under `key` (XChaCha20-Poly1305, kete).
pub fn seal(book: &AddrBook, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    kete::encrypt_bytes(&encode(book)?, key)
}

/// Open a sealed book under `key`; a book sealed under any other key does not open.
pub fn open(sealed: &[u8], key: &[u8; 32]) -> Result<AddrBook, String> {
    decode(&kete::decrypt_bytes(sealed, key)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DevicePubkey;
    use ed25519_dalek::SigningKey;

    fn signed(seed: u8, hp: [u8; 32], ip: &str, at: i64) -> PeerRecord {
        let sk = SigningKey::from_bytes(&[seed; 32]);
        let mut r = PeerRecord::new(hp, DevicePubkey::from_bytes(sk.verifying_key().to_bytes()), ip.parse().unwrap());
        r.last_seen = at;
        r.local_ip = Some("192.168.1.7".parse().unwrap());
        r.local_port = 4383;
        r.sign(&sk);
        r
    }

    /// A book round-trips sealed: every record arrives with its signature intact (it still verifies by the device it describes), every heard address with its identity, device, address and time; another key opens nothing.
    #[test]
    fn a_sealed_book_round_trips_and_its_records_still_verify() {
        let book = AddrBook {
            records: vec![signed(3, [0x11; 32], "203.0.113.9:4383", 1000), signed(4, [0x22; 32], "[2001:db8::7]:4383", 2000)],
            heard: vec![
                HeardAddr { handle_proof: [0x11; 32], device: [9; 32], addr: "198.51.100.4:51000".parse().unwrap(), heard: 77 },
                HeardAddr { handle_proof: [0x33; 32], device: [8; 32], addr: "[2001:db8::9]:4383".parse().unwrap(), heard: 78 },
            ],
        };
        let key = [0x5a; 32];
        let sealed = seal(&book, &key).expect("seal");
        let got = open(&sealed, &key).expect("open");
        assert_eq!(got.records.len(), 2);
        for (a, b) in got.records.iter().zip(&book.records) {
            assert!(a.verify(), "a handed-over record verifies by its own device's signature");
            assert_eq!((a.handle_proof, a.device_pubkey.as_bytes(), a.ip, a.local_ip, a.local_port, a.last_seen), (b.handle_proof, b.device_pubkey.as_bytes(), b.ip, b.local_ip, b.local_port, b.last_seen));
        }
        assert_eq!(got.heard, book.heard);
        assert!(open(&sealed, &[0x5b; 32]).is_err(), "another sibling's key opens nothing");
    }

    /// A v4-mapped heard address travels as plain v4, the form every reader compares in.
    #[test]
    fn a_mapped_address_travels_canonical() {
        let book = AddrBook { records: vec![], heard: vec![HeardAddr { handle_proof: [1; 32], device: [2; 32], addr: "[::ffff:192.168.1.245]:4383".parse().unwrap(), heard: 5 }] };
        let got = decode(&encode(&book).unwrap()).unwrap();
        assert_eq!(got.heard[0].addr, "192.168.1.245:4383".parse::<SocketAddr>().unwrap());
    }
}
