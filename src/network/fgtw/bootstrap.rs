use super::{fingerprint::Keypair, PeerRecord};
use crate::types::DevicePubkey;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use vsf::schema::FromVsfType;

use crate::network::http::SEED_HTTPS as FGTW_URL;

/// Result of a bootstrap query. `peers` is now always EMPTY: the announce acks rather than echoing the phonebook, so the seed no longer supplies peers -- they come from the persisted store and gossip, with fgtw.org as the last resort. The field stays so callers keep their shape (they filter and merge, which is a no-op on empty); `error` still carries any transport or verification failure.
#[derive(Debug)]
pub struct BootstrapResult {
    pub peers: Vec<PeerRecord>,
    pub error: Option<String>,
}

// FGTW Seed Public Keys (hardcoded to avoid extra queries) X25519 public key - for encrypting announce messages
pub const FGTW_X25519_PUBLIC_KEY: [u8; 32] = [
    0xD6, 0x0B, 0x9D, 0xAC, 0x7F, 0x3F, 0x9D, 0x0E, 0xFC, 0xC2, 0x87, 0x88, 0xFB, 0x55, 0x56, 0x95, 
    0x1E, 0x47, 0x95, 0x63, 0xB0, 0x74, 0xE8, 0xD1, 0x40, 0xE7, 0xDD, 0x51, 0x21, 0xCA, 0xE4, 0x24, 
];

// Ed25519 public key - for verifying challenge signatures
pub const FGTW_ED25519_PUBLIC_KEY: [u8; 32] = [
    0x02, 0x1C, 0xDF, 0x80, 0x43, 0x0C, 0x09, 0xFD, 0x58, 0xA4, 0xF7, 0xCD, 0x86, 0x03, 0x78, 0x0A, 
    0xFC, 0x30, 0x87, 0xEF, 0x16, 0x24, 0x3F, 0xC1, 0x63, 0x9D, 0x31, 0x5F, 0x94, 0x06, 0xEB, 0x6D, 
];

/// Try to parse a VSF error message from response bytes Returns Some(error_message) if the response is a worker `error` frame, None otherwise. The old hand-rolled scan for legacy "message"/"error" section shapes is retired — the worker answers every failure as a `{reason, detail}` error frame (fgtw 6b01e46).
fn try_parse_vsf_error(bytes: &[u8]) -> Option<String> {
    fgtw::client::error_frame(bytes)
        .map(|(reason, detail)| if detail.is_empty() { reason } else { detail })
}

/// Turn an FGTW error response into a SHORT, plain message with no web-stack jargon (no status numbers, no "Bad Request"/"Internal Server Error" reason phrases, no URLs). FGTW signs its own error reasons; if one is present we surface that (it's ours and it's meaningful); otherwise the message is a plain "FGTW couldn't <step>" split only by whether the fault is on their side (5xx) or ours (4xx). The raw HTTP terminology is a transport detail the user can't act on.
/// Body-from-bytes variant — used by the announce path where the body was already buffered for VSF-error parsing before falling thru here.
fn format_http_error_from_bytes(step: &str, status: reqwest::StatusCode, body: &[u8]) -> String {
    if let Some(msg) = try_parse_vsf_error(body) {
        return format!("FGTW: {msg}");
    }
    if status.is_server_error() {
        format!("FGTW is having trouble — couldn't {step}")
    } else {
        format!("FGTW rejected {step}")
    }
}

/// Turn a worker `error`-frame `(reason, detail)` into a short user-facing message. The worker now answers every failure this way at HTTP 200; the `detail` string is already plain (no web-stack jargon), so surface it verbatim, keeping the operation `step` for context.
fn reason_error(step: &str, reason: &str, detail: &str) -> String {
    if detail.is_empty() {
        format!("FGTW rejected {step} ({reason})")
    } else {
        format!("FGTW: {detail}")
    }
}

/// Load bootstrap peers by announcing to FGTW This requires authenticating with our handle and device key Returns BootstrapResult which includes peers even on error (for peer discovery)
///
/// # Arguments
/// * `device_key` - Device's Ed25519 keypair * `handle_proof` - Handle proof hash * `port` - Local P2P port * `identity_seed` - The owner's `ihi::handle_to_hash` root (for avatar keypair derivation; no handle string)
pub async fn load_bootstrap_peers(
    device_key: &Keypair,
    handle_proof: [u8; 32],
    port: u16,
    identity_seed: &[u8; 32],
) -> BootstrapResult {
    match load_bootstrap_peers_inner(device_key, handle_proof, port, identity_seed).await {
        Ok(peers) => BootstrapResult { peers, error: None },
        Err(e) => BootstrapResult {
            peers: vec![],
            error: Some(e),
        },
    }
}

/// Inner implementation that returns Result for easier error handling
async fn load_bootstrap_peers_inner(
    device_key: &Keypair,
    handle_proof: [u8; 32],
    port: u16,
    identity_seed: &[u8; 32],
) -> Result<Vec<PeerRecord>, String> {
    // Shared async client — pools on the process-wide runtime, so the TLS session is reused across announces (challenge + announce here are two requests on one warm connection). The per-request `.timeout(10s)` below preserves the old client-level budget.
    let client = crate::network::http::async_client();

    // Ensure this device's fleet membership BEFORE announcing — a fresh identity claims its fleet with a first-come, identity-signed genesis, so the membership-gated announce below (and avatar writes) are authorised.
    // The fleet client uses the blocking HTTP path, so bridge thru spawn_blocking rather than calling it from this async context.
    {
        let dk = device_key.clone();
        let seed = *identity_seed;
        tokio::task::spawn_blocking(move || {
            crate::network::fgtw::fleet::ensure_member(&dk, &handle_proof, &seed)
        })
        .await
        .map_err(|_| "fleet setup interrupted".to_string())?
        // ensure_member already returns short, plain messages ("No connection to FGTW", "this device is not in the fleet — …") — surface them as-is, no prefix.
        ?;
    }

    // Get challenge from FGTW (POST / with VSF section "challenge")
    let challenge_vsf = {
        let unsigned = vsf::VsfBuilder::new()
            .creation_time_oscillations(vsf::eagle_time_oscillations())
            .add_section("challenge", vec![])
            .build()
            .map_err(|e| format!("Build challenge request: {}", e))?;
        unsigned
    };

    let challenge_response = client
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(10))
        .header("Content-Type", "application/octet-stream")
        .body(challenge_vsf)
        .send()
        .await
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;

    let challenge_status = challenge_response.status();
    let challenge_bytes = challenge_response
        .bytes()
        .await
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;

    // The worker answers every failure with a VSF `error` frame at HTTP 200; surface its reason.
    if let Some((reason, detail)) = fgtw::client::error_frame(&challenge_bytes) {
        return Err(reason_error("challenge", &reason, &detail));
    }
    if !challenge_status.is_success() {
        return Err(format_http_error_from_bytes("challenge", challenge_status, &challenge_bytes));
    }

    #[cfg(feature = "development")]
    crate::log(&crate::network::inspect::vsf_inspect(
        &challenge_bytes,
        "FGTW",
        "RX",
        "challenge",
    ));

    // Parse challenge to extract provenance hash
    let challenge_hash = parse_challenge_hash(&challenge_bytes)?;

    // Derive avatar keypair for authentication
    let (_, avatar_verifying_key) =
        crate::avatar::derive_avatar_keypair_from_seed(&device_key.secret, identity_seed);
    let avatar_pub_key = Some(*avatar_verifying_key.as_bytes());

    // Build announce message with challenge response and avatar pubkey
    let announce_bytes = build_announce_message(
        handle_proof,
        device_key,
        port,
        challenge_hash,
        avatar_pub_key,
    )?;

    // Send announce to FGTW
    let announce_response = client
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(10))
        .header("Content-Type", "application/octet-stream")
        .body(announce_bytes)
        .send()
        .await
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;

    let status = announce_response.status();

    let response_bytes = announce_response
        .bytes()
        .await
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;

    #[cfg(feature = "development")]
    crate::log(&crate::network::inspect::vsf_inspect(
        &response_bytes,
        "FGTW",
        "RX",
        "announce",
    ));

    // App-level failure first: the worker answers every failure with a VSF `error` frame at HTTP 200 (`not_fleet_member`, `bad_signature`, …). Fall back to the legacy VSF-error / transport phrasing only if it isn't one of the new reason frames.
    if let Some((reason, detail)) = fgtw::client::error_frame(&response_bytes) {
        return Err(reason_error("announce", &reason, &detail));
    }
    if !status.is_success() {
        if let Some(error_msg) = try_parse_vsf_error(&response_bytes) {
            return Err(error_msg);
        }
        return Err(format_http_error_from_bytes(
            "announce",
            status,
            &response_bytes,
        ));
    }

    // The announce ACKS; it no longer echoes the phonebook. The seed is the LAST resort for peer
    // discovery, not the first -- our own store persists and gossips, so a seed-supplied list was
    // duplicating what we already hold, at the cost of a full per-recipient encrypt on every attest.
    // What we still want from the ack is the reflexive address the worker observed, which is the one
    // thing only the server can tell us.
    let observed = parse_announce_ack(&response_bytes)?;
    crate::logf!("FGTW: announce ok, {} identities known, we look like {}", observed.1, observed.0);

    Ok(Vec::new())
}

/// Verify the `announce_ok` ack and pull out (observed address, identity count).
///
/// Verified read pinned to the FGTW signing key, same as the challenge: an ack that isn't signed by FGTW
/// tells us nothing about what the network saw. There is no fallback to the old `encrypted_peers` shape --
/// a worker still serving it fails here loudly, which is the intent (AGENT.md "No Fork Bullshit").
fn parse_announce_ack(bytes: &[u8]) -> Result<(String, u32), String> {
    let schema = vsf::schema::SectionSchema::new("announce_ok")
        .field("count", vsf::schema::TypeConstraint::Any)
        .field("ip", vsf::schema::TypeConstraint::Any)
        .field("port", vsf::schema::TypeConstraint::Any);
    let section = vsf::schema::SectionBuilder::parse_document(schema, bytes, Some(FGTW_ED25519_PUBLIC_KEY))
        .map_err(|e| format!("Verified parse of announce_ok: {}", e))?;
    let count: u32 = section.get_value("count").unwrap_or(0);
    let ip: String = section.get_value("ip").unwrap_or_default();
    let port: u16 = section.get_value("port").unwrap_or(0);
    Ok((format!("{ip}:{port}"), count))
}

/// Parse challenge VSF to extract provenance hash The timestamp in the challenge is ignored - announce generates its own timestamp
fn parse_challenge_hash(bytes: &[u8]) -> Result<[u8; 32], String> {
    use vsf::VsfType;

    // Verified read pinned to the FGTW signing key: is_original + Ed25519(ge over BLAKE3(file, ge zeroed)) + ke must equal FGTW_ED25519_PUBLIC_KEY. A challenge that fails ANY of those is not from FGTW.
    let (header, _header_len) =
        vsf::verification::read_verified(bytes, Some(FGTW_ED25519_PUBLIC_KEY))
            .map_err(|e| format!("Challenge verification failed - not from authentic FGTW: {}", e))?;

    // The provenance hash is the challenge value.
    match &header.provenance_hash {
        VsfType::hp(hash) if hash.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(hash);
            Ok(arr)
        }
        VsfType::hp(hash) => Err(format!("Invalid provenance hash length: {}", hash.len())),
        _ => Err("Invalid provenance hash type".to_string()),
    }
}

/// Encrypt data for FGTW using ephemeral X25519 + AES-256-GCM Format: [ephemeral_pubkey:32][nonce:12][ciphertext+tag] This matches FGTW's Web Crypto API implementation
fn encrypt_for_fgtw(plaintext: &[u8], fgtw_x25519_pubkey: &[u8; 32]) -> Result<Vec<u8>, String> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use rand::rngs::OsRng;
    use x25519_dalek::{EphemeralSecret, PublicKey};

    // Use the X25519 public key directly
    let x25519_pubkey = PublicKey::from(*fgtw_x25519_pubkey);

    // Generate ephemeral X25519 keypair
    let ephemeral_secret = EphemeralSecret::random_from_rng(OsRng);
    let ephemeral_public = PublicKey::from(&ephemeral_secret);

    // Perform ECDH with FGTW's X25519 public key
    let shared_secret = ephemeral_secret.diffie_hellman(&x25519_pubkey);

    // Derive AES-256-GCM key from shared secret (32 bytes)
    let cipher = Aes256Gcm::new(shared_secret.as_bytes().into());

    // Generate random nonce (12 bytes for AES-GCM)
    let mut nonce_bytes = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);

    // Encrypt
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| format!("Encryption error: {}", e))?;

    // Combine: ephemeral_pubkey || nonce || ciphertext+tag
    let mut result = Vec::new();
    result.extend_from_slice(ephemeral_public.as_bytes());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}



/// Build VSF announce message (new encrypted format) Structure: RÅ< z y b ef6 hp ke ge n[1] (d"announce" o b n) > [announce payload] The device Ed25519 key (ke) and signature (ge) are at HEADER level for full file integrity
fn build_announce_message(
    handle_proof: [u8; 32],
    device_key: &Keypair,
    port: u16,
    challenge_hash: [u8; 32],
    avatar_pub_key: Option<[u8; 32]>,
) -> Result<Vec<u8>, String> {
    use vsf::verification::sign_file;
    use vsf::{VsfBuilder, VsfType};

    // 1. Build encrypted payload: hb(challenge_hash) + hP(handle_proof) + u(port) + t_u3(local_ip)? + ke(avatar_pub)?
    let mut plaintext = Vec::new();
    plaintext.extend(VsfType::hb(challenge_hash.to_vec()).flatten());
    plaintext.extend(VsfType::hP(handle_proof.to_vec()).flatten());
    plaintext.extend(VsfType::u(port as usize, false).flatten());

    // Include local IP for hairpin NAT (peers behind same public IP)
    if let Some(local_ip) = crate::network::udp::get_local_ip() {
        let octets = local_ip.octets();
        plaintext.extend(VsfType::t_u3(vsf::Tensor::new(vec![4], octets.to_vec())).flatten());
    }

    // Optional: include avatar public key for avatar authentication
    if let Some(avatar_key) = avatar_pub_key {
        plaintext.extend(VsfType::ke(avatar_key.to_vec()).flatten());
    }

    // 2. Encrypt for FGTW using ephemeral X25519 + AES-GCM
    let encrypted = encrypt_for_fgtw(&plaintext, &FGTW_X25519_PUBLIC_KEY)?;

    // 3. Build VSF with ke/ge at HEADER level (not inside section) for full file integrity
    let unsigned_bytes = VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only(VsfType::ke(device_key.public.to_bytes().to_vec()))
        .add_section(
            "announce",
            vec![("payload".to_string(), VsfType::v(b'e', encrypted))],
        )
        .build()?;

    // 4. Sign the entire file (header-level signature)
    let vsf_bytes = sign_file(unsigned_bytes, device_key.secret.as_bytes())?;

    #[cfg(feature = "development")]
    crate::log(&crate::network::inspect::vsf_inspect(
        &vsf_bytes, "FGTW", "TX", "announce",
    ));

    Ok(vsf_bytes)
}


/// Parse a PeerRecord from a VsfField Expected format: (peer: hb{32}, ke{32}, t_u3{IP}, u3{port}, ef6{timestamp})
pub(crate) fn parse_peer_from_field(field: &vsf::VsfField) -> Result<PeerRecord, String> {
    if field.values.len() < 5 {
        return Err(format!(
            "Peer field needs 5 values, got {}",
            field.values.len()
        ));
    }

    // Parse handle_proof (hP{32})
    let handle_proof = match &field.values[0] {
        vsf::VsfType::hP(h) if h.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(h);
            arr
        }
        _ => return Err("Invalid handle_proof type or length".to_string()),
    };

    // Parse device_pubkey (ke{32})
    let device_pubkey = match &field.values[1] {
        vsf::VsfType::ke(k) if k.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(k);
            DevicePubkey::from_bytes(arr)
        }
        _ => return Err("Invalid device_pubkey type or length".to_string()),
    };

    // Parse IP address (t_u3{4 or 16 bytes})
    let ip_bytes = match &field.values[2] {
        vsf::VsfType::t_u3(tensor) => &tensor.data,
        _ => return Err("Invalid ip type".to_string()),
    };

    let parsed_ip = if ip_bytes.len() == 4 {
        IpAddr::V4(Ipv4Addr::new(
            ip_bytes[0],
            ip_bytes[1],
            ip_bytes[2],
            ip_bytes[3],
        ))
    } else if ip_bytes.len() == 16 {
        let mut octets = [0u8; 16];
        octets.copy_from_slice(ip_bytes);
        IpAddr::V6(Ipv6Addr::from(octets))
    } else {
        return Err(format!("Invalid IP length: {}", ip_bytes.len()));
    };

    // Parse port (u3 or generic u)
    let port = u16::from_vsf_type(&field.values[3]).map_err(|e| format!("Invalid port: {}", e))?;

    // Parse timestamp (Eagle Time oscillations)
    let last_seen = match &field.values[4] {
        vsf::VsfType::e(vsf::types::EtType::e6(osc)) => *osc,
        _ => return Err("Expected Eagle Time i64 oscillations for timestamp".to_string()),
    };

    // Parse optional local_ip (t_u3{4 or 16 bytes}) for hairpin NAT
    let local_ip = if field.values.len() > 5 {
        match &field.values[5] {
            vsf::VsfType::t_u3(tensor) if tensor.data.len() == 4 => {
                Some(IpAddr::V4(Ipv4Addr::new(
                    tensor.data[0],
                    tensor.data[1],
                    tensor.data[2],
                    tensor.data[3],
                )))
            }
            vsf::VsfType::t_u3(tensor) if tensor.data.len() == 16 => {
                let mut octets = [0u8; 16];
                octets.copy_from_slice(&tensor.data);
                Some(IpAddr::V6(Ipv6Addr::from(octets)))
            }
            _ => None,
        }
    } else {
        None
    };

    // Parse optional self-signature (ge{64}) at index 6.
    // A record without it (or with a bad one) is left unsigned; merge_peer's verify() drops unsigned records, so only properly self-signed entries propagate.
    // FGTW-sourced records carry it once the server serves the signed form.
    let signature = if field.values.len() > 6 {
        match &field.values[6] {
            vsf::VsfType::ge(s) if s.len() == 64 => s.as_slice().try_into().unwrap(),
            _ => [0u8; 64],
        }
    } else {
        [0u8; 64]
    };

    Ok(PeerRecord {
        handle_proof,
        device_pubkey,
        ip: SocketAddr::new(parsed_ip, port),
        local_ip,
        last_seen,
        signature,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an `announce_ok` ack byte-for-byte the way the worker does, then read it with the client's
    /// own parser. This pins the two halves of a contract that live in different repos.
    ///
    /// It exists because the first cut of this ack was UNSIGNED: `vsf_bytes_response` doesn't sign, and
    /// only `handle_challenge` filled a `ge`. The client verifies pinned to the FGTW key, so every attest
    /// would have failed at `read_verified` -- after deploy, on all five clients, with the seed being the
    /// only way to attest. A round-trip test is the cheap way to catch a cross-repo shape mismatch.
    fn worker_shaped_ack(signer: &ed25519_dalek::SigningKey, count: u32, ip: &str, port: u16) -> Vec<u8> {
        use vsf::VsfType;
        let unsigned = vsf::VsfBuilder::new()
            .creation_time_oscillations(vsf::eagle_time_oscillations())
            .signed_only(VsfType::ke(signer.verifying_key().to_bytes().to_vec()))
            .add_section(
                "announce_ok",
                vec![
                    ("count".to_string(), VsfType::u5(count)),
                    ("ip".to_string(), VsfType::a(ip.to_string())),
                    ("port".to_string(), VsfType::u4(port)),
                ],
            )
            .build()
            .expect("build ack");
        let hp = vsf::verification::compute_provenance_hash(&unsigned).expect("hp");
        let mut bytes = unsigned;
        vsf::verification::fill_provenance_hash(&mut bytes, &hp).expect("fill hp");
        // ge signs BLAKE3(file with hp filled, ge zeroed) — the canonical scheme read_verified enforces.
        let file_hash = blake3::hash(&bytes);
        use ed25519_dalek::Signer;
        let sig = signer.sign(file_hash.as_bytes());
        vsf::verification::fill_signature(&mut bytes, &sig.to_bytes()).expect("fill ge");
        bytes
    }

    #[test]
    fn announce_ack_round_trips_from_a_worker_shaped_document() {
        let sk = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        let bytes = worker_shaped_ack(&sk, 7, "203.0.113.9", 4383);

        // Parse with the pin the real client uses, but pointed at THIS test key.
        let schema = vsf::schema::SectionSchema::new("announce_ok")
            .field("count", vsf::schema::TypeConstraint::Any)
            .field("ip", vsf::schema::TypeConstraint::Any)
            .field("port", vsf::schema::TypeConstraint::Any);
        let section = vsf::schema::SectionBuilder::parse_document(
            schema,
            &bytes,
            Some(sk.verifying_key().to_bytes()),
        )
        .expect("a worker-shaped ack must parse under the pinned key");

        assert_eq!(section.get_value::<u32>("count").expect("count"), 7);
        assert_eq!(section.get_value::<String>("ip").expect("ip"), "203.0.113.9");
        assert_eq!(section.get_value::<u16>("port").expect("port"), 4383);
    }

    /// An ack from the WRONG key must be refused. The reflexive address in it is the one thing only the
    /// server can assert, so accepting an unpinned ack would let anything on the path tell us we look
    /// like an address it chose.
    #[test]
    fn announce_ack_from_a_foreign_key_is_refused() {
        let real = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        let imposter = ed25519_dalek::SigningKey::from_bytes(&[43u8; 32]);
        let bytes = worker_shaped_ack(&imposter, 1, "198.51.100.4", 4383);

        let schema = vsf::schema::SectionSchema::new("announce_ok")
            .field("count", vsf::schema::TypeConstraint::Any);
        assert!(
            vsf::schema::SectionBuilder::parse_document(schema, &bytes, Some(real.verifying_key().to_bytes())).is_err(),
            "an ack signed by a foreign key must not verify"
        );
    }

    /// An UNSIGNED ack must be refused — the exact defect this module's first cut shipped with.
    #[test]
    fn unsigned_announce_ack_is_refused() {
        use vsf::VsfType;
        let bytes = vsf::VsfBuilder::new()
            .creation_time_oscillations(vsf::eagle_time_oscillations())
            .add_section("announce_ok", vec![("count".to_string(), VsfType::u5(3))])
            .build()
            .expect("build unsigned");
        let schema = vsf::schema::SectionSchema::new("announce_ok")
            .field("count", vsf::schema::TypeConstraint::Any);
        let sk = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        assert!(
            vsf::schema::SectionBuilder::parse_document(schema, &bytes, Some(sk.verifying_key().to_bytes())).is_err(),
            "an ack with no ge must not verify against a pinned key"
        );
    }
}
