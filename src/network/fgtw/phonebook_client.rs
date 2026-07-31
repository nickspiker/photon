//! Phonebook registry client — publish our own address record to the seed, and resolve another device's address from it.
//!
//! This is the replacement for the announce echo. The old `announce` response carried the whole peer list, which is how every contact learned an address on a cold start; dropping it is what made the per-record registry cutover possible (there is no aggregate blob to echo once records are addressed individually), but until this module existed there was NO seed-side discovery at all — only gossip against a store that starts empty, over paths that need an address to establish.
//!
//! The wire is deliberately thin: a record is 256 self-describing bytes carrying its own signature, so it crosses as raw bytes and is verified by the RECEIVER against the codec both sides share ([`fgtw::phonebook`]). Nothing here is trusted because the seed said it — a record that fails `verify_address` is dropped exactly as if it had never arrived.
//!
//! What this does NOT do: prove the device belongs to the identity's fleet. `verify_address` proves only that the holder of `device_pubkey` signed this address under this `handle_proof`; a stranger can mint a row for any scraped `handle_proof`. That binding is the PRIMARY registry's job and is still open — see [`resolve_device_address`] for what a caller may and may not conclude.

use fgtw::phonebook::{key_address, Key, Record, STRIDE};
use vsf::VsfType;

use crate::network::http::SEED_HTTPS as FGTW_URL;

/// How long a phonebook call may take. Short: this runs on the address-discovery path, where a slow answer is worse than no answer (the caller falls back to gossip/relay, which still works).
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug)]
pub enum PhonebookError {
    Network(String),
    NotFound,
    Rejected(String),
    /// The seed returned a record whose own signature does not verify. Treated as absence, never as data — a seed that serves a forged row must not be able to redirect anyone.
    Unverified,
}

impl std::fmt::Display for PhonebookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PhonebookError::Network(s) => write!(f, "network: {}", s),
            PhonebookError::NotFound => write!(f, "no record"),
            PhonebookError::Rejected(s) => write!(f, "rejected: {}", s),
            PhonebookError::Unverified => write!(f, "record failed verification"),
        }
    }
}

async fn post(body: Vec<u8>) -> Result<Vec<u8>, PhonebookError> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| PhonebookError::Network(format!("client: {}", e)))?;
    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(body)
        .send()
        .await
        .map_err(|e| PhonebookError::Network(format!("post: {}", e)))?;
    let status = response.status();
    let bytes = response.bytes().await.unwrap_or_default().to_vec();
    if let Some((reason, detail)) = fgtw::client::error_frame(&bytes) {
        return Err(match reason.as_str() {
            "not_found" => PhonebookError::NotFound,
            _ => PhonebookError::Rejected(format!("{reason}: {detail}")),
        });
    }
    if !status.is_success() {
        return Err(PhonebookError::Network(format!("transport {}", status)));
    }
    Ok(bytes)
}

/// Publish OUR OWN signed address record so other devices can find us without gossip.
///
/// The record is signed by this device's key over `(handle_proof ‖ device_pubkey ‖ ip ‖ port ‖ local_ip ‖ timestamp)`, so the seed stores a claim it cannot forge or alter. `timestamp` is the monotonic guard: the seed refuses a record no newer than the one it holds, which stops a captured row being replayed to roll our address back to somewhere an attacker can reach.
///
/// The address published must be one we actually believe in — a quorum-corroborated reflexive address, never the relay sentinel. The caller owns that check; signing an unreachable address produces a signed claim that we cannot be reached, and gossip would propagate it faithfully.
pub async fn publish_address(
    device_secret: &ed25519_dalek::SigningKey,
    handle_proof: &[u8; 32],
    addr: std::net::SocketAddr,
    local_ip: Option<std::net::IpAddr>,
) -> Result<(), PhonebookError> {
    let rec = Record::sign_device_address(
        device_secret,
        handle_proof,
        &ip_to_bytes(addr.ip()),
        addr.port(),
        &local_ip.map(ip_to_bytes).unwrap_or([0u8; 16]),
        vsf::eagle_time_oscillations(),
    );
    debug_assert!(rec.verify_address(), "a record we just signed must verify");

    let body = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section(
            "pb_put",
            vec![("rec".to_string(), VsfType::v(b'r', rec.0.to_vec()))],
        )
        .build()
        .map_err(|e| PhonebookError::Network(format!("build: {}", e)))?;
    post(body).await?;
    Ok(())
}

/// Resolve one device's published address.
///
/// The returned record is signature-verified HERE, so the caller can rely on "the holder of `device_pubkey` claims this address". The caller may NOT conclude the device belongs to any particular fleet — `handle_proof` is inside the signature, so the device did claim it, but nothing yet proves a current member of that fleet ever vouched for the device. Use it to ADDRESS a device you already have reason to care about (a contact's known device pubkey), not to accept a stranger into a fleet.
pub async fn resolve_device_address(device_pubkey: &[u8; 32]) -> Result<Record, PhonebookError> {
    let key: Key = key_address(device_pubkey);
    let body = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section(
            "pb_get",
            vec![("key".to_string(), VsfType::v(b'r', key.to_vec()))],
        )
        .build()
        .map_err(|e| PhonebookError::Network(format!("build: {}", e)))?;
    let bytes = post(body).await?;

    let rec = parse_pb_rec(&bytes).ok_or(PhonebookError::NotFound)?;
    // Verify before returning: a record that fails is absence, not data.
    if !rec.verify_address() {
        return Err(PhonebookError::Unverified);
    }
    // The record must be the one we asked for. Without this a seed could answer any query with any valid record and silently redirect us to a device we never asked about.
    if rec.key() != Some(key) {
        return Err(PhonebookError::Unverified);
    }
    Ok(rec)
}

/// Pull the raw 256-byte record out of a `pb_rec` response frame. Verified read: header + provenance self-consistency before any field is trusted; the record's own signature is checked by the caller on top.
fn parse_pb_rec(bytes: &[u8]) -> Option<Record> {
    let schema = vsf::schema::SectionSchema::new("pb_rec")
        .field("rec", vsf::schema::TypeConstraint::Any);
    let section = vsf::schema::SectionBuilder::parse_document(schema, bytes, None).ok()?;
    match section.get_fields("rec").first().and_then(|f| f.values.first()) {
        Some(VsfType::v(_, b)) if b.len() == STRIDE => {
            let mut a = [0u8; STRIDE];
            a.copy_from_slice(b);
            Some(Record(a))
        }
        _ => None,
    }
}

/// v4 as `::ffff:a.b.c.d` so the field is fixed width — the record layout stores one 16-byte slot for either family.
fn ip_to_bytes(ip: std::net::IpAddr) -> [u8; 16] {
    match ip {
        std::net::IpAddr::V4(v4) => v4.to_ipv6_mapped().octets(),
        std::net::IpAddr::V6(v6) => v6.octets(),
    }
}

/// The inverse of [`ip_to_bytes`]: unwrap a v4-mapped address back to a real v4 so it compares equal to addresses learned off the wire (a `::ffff:` form would never match a stored v4 and would be punched at as a v6 host).
pub fn bytes_to_ip(b: &[u8; 16]) -> std::net::IpAddr {
    let v6 = std::net::Ipv6Addr::from(*b);
    match v6.to_ipv4_mapped() {
        Some(v4) => std::net::IpAddr::V4(v4),
        None => std::net::IpAddr::V6(v6),
    }
}

/// The socket address a resolved record points at, or `None` when the record carries no usable address (an all-zero IP is the unwritten/absent case, and adopting it would send to a black hole).
pub fn record_socket_addr(rec: &Record) -> Option<std::net::SocketAddr> {
    let ip = bytes_to_ip(&rec.ip());
    if ip.is_unspecified() || rec.port() == 0 {
        return None;
    }
    Some(std::net::SocketAddr::new(ip, rec.port()))
}

/// The LAN address a resolved record carries, if any.
pub fn record_local_addr(rec: &Record) -> Option<std::net::SocketAddr> {
    let ip = bytes_to_ip(&rec.local_ip());
    if ip.is_unspecified() || rec.port() == 0 {
        return None;
    }
    Some(std::net::SocketAddr::new(ip, rec.port()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[seed; 32])
    }

    /// A v4 address must survive the mapped-form round trip as a REAL v4 — if it came back as `::ffff:a.b.c.d` it would never compare equal to the same address learned off the wire, and the punch would treat it as a v6 host that needs no hole.
    #[test]
    fn a_v4_address_round_trips_unmapped() {
        let v4: std::net::IpAddr = "192.168.0.42".parse().unwrap();
        assert_eq!(bytes_to_ip(&ip_to_bytes(v4)), v4);
        let v6: std::net::IpAddr = "2600:100f:b0c8:2029::1".parse().unwrap();
        assert_eq!(bytes_to_ip(&ip_to_bytes(v6)), v6);
    }

    /// The full publish-side shape: what we sign is what a resolver reads back, addresses intact.
    #[test]
    fn a_published_record_reads_back_with_its_addresses() {
        let dev = key(1);
        let addr: std::net::SocketAddr = "66.135.65.206:4383".parse().unwrap();
        let lan: std::net::IpAddr = "192.168.0.40".parse().unwrap();
        let rec = Record::sign_device_address(
            &dev,
            &[3u8; 32],
            &ip_to_bytes(addr.ip()),
            addr.port(),
            &ip_to_bytes(lan),
            77,
        );
        assert!(rec.verify_address());
        assert_eq!(record_socket_addr(&rec), Some(addr));
        assert_eq!(
            record_local_addr(&rec),
            Some(std::net::SocketAddr::new(lan, 4383))
        );
    }

    /// An all-zero address slot is ABSENCE, not `0.0.0.0` — adopting it as a peer endpoint is the relay-sentinel poisoning that made sends go nowhere while `validated_path` looked Some.
    #[test]
    fn an_empty_address_slot_is_absence() {
        let dev = key(2);
        let rec = Record::sign_device_address(&dev, &[3u8; 32], &[0u8; 16], 0, &[0u8; 16], 5);
        assert_eq!(record_socket_addr(&rec), None);
        assert_eq!(record_local_addr(&rec), None);
    }

    /// A response frame carrying a record for a DIFFERENT device must be refused by the key check in `resolve_device_address` — the parse itself is agnostic, so pin that the keys differ and the comparison is the thing that catches it.
    #[test]
    fn a_record_for_another_device_has_a_different_key() {
        let ours = key(1);
        let theirs = key(9);
        let rec = Record::sign_device_address(&theirs, &[3u8; 32], &[1u8; 16], 4383, &[0u8; 16], 5);
        assert!(
            rec.verify_address(),
            "their record is genuinely valid — validity is not enough"
        );
        assert_ne!(
            rec.key(),
            Some(key_address(&ours.verifying_key().to_bytes())),
            "a valid record for the wrong device must not satisfy our query"
        );
    }

    /// LIVE smoke check against the deployed seed — `#[ignore]`d so it never runs in the normal suite (it needs the network and a deployed worker). Run explicitly after a deploy: `cargo test --lib phonebook_client -- --ignored --nocapture`
    ///
    /// Asserts the ROUTE exists: an unknown section answers `unknown_op`, so a `not_found` for a key that cannot exist is the positive result.
    #[test]
    #[ignore]
    fn live_seed_answers_a_phonebook_lookup() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let missing = [0x5Au8; 32];
        let res = rt.block_on(resolve_device_address(&missing));
        match res {
            Err(PhonebookError::NotFound) => {}
            Err(PhonebookError::Rejected(m)) if m.starts_with("unknown_op") => {
                panic!("pb_get route is NOT deployed: {m}")
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// LIVE end-to-end: publish a record for a throwaway device key, then resolve it back and confirm the address survives the round trip through R2. `#[ignore]`d — network + deploy. Uses a key derived from the current time so repeat runs never fight the monotonic guard.
    #[test]
    #[ignore]
    fn live_publish_then_resolve_round_trips() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let mut seed = [0u8; 32];
        seed[..8].copy_from_slice(&vsf::eagle_time_oscillations().to_le_bytes());
        let dev = ed25519_dalek::SigningKey::from_bytes(&seed);
        let pubkey = dev.verifying_key().to_bytes();
        let addr: std::net::SocketAddr = "203.0.113.7:4383".parse().unwrap();
        let lan: std::net::IpAddr = "192.168.7.7".parse().unwrap();

        rt.block_on(publish_address(&dev, &[0xABu8; 32], addr, Some(lan)))
            .expect("publish succeeds");
        let rec = rt
            .block_on(resolve_device_address(&pubkey))
            .expect("resolve finds it");
        assert!(rec.verify_address());
        assert_eq!(
            record_socket_addr(&rec),
            Some(addr),
            "the public address round trips"
        );
        assert_eq!(
            rec.handle_proof(),
            [0xABu8; 32],
            "the signed handle_proof round trips"
        );
        println!(
            "live round trip OK: {} lan={:?}",
            addr,
            record_local_addr(&rec)
        );
    }

    /// The exact frame the worker's `pb_get` builds must parse here. This is the wire contract between the two halves; a type or stride change on either side breaks discovery silently.
    #[test]
    fn a_pb_rec_frame_parses_back_to_the_record() {
        let dev = key(4);
        let rec = Record::sign_device_address(&dev, &[3u8; 32], &[7u8; 16], 4383, &[8u8; 16], 12);
        let frame = vsf::VsfBuilder::new()
            .creation_time_oscillations(vsf::eagle_time_oscillations())
            .add_section(
                "pb_rec",
                vec![("rec".to_string(), VsfType::v(b'r', rec.0.to_vec()))],
            )
            .build()
            .expect("frame builds");
        let parsed = parse_pb_rec(&frame).expect("frame parses");
        assert_eq!(parsed.0, rec.0);
        assert!(parsed.verify_address());
    }
}
