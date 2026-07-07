use super::{fingerprint::Keypair, PeerRecord};
use crate::types::DevicePubkey;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use vsf::{schema::FromVsfType, VsfSection};

const FGTW_URL: &str = "https://fgtw.org";

/// Result of a bootstrap query. `peers` carries whatever records parsed successfully; a malformed
/// record is skipped (not fatal) rather than aborting the whole list, and a transport/decode-level
/// failure is reported in `error` while still returning any peers already recovered.
#[derive(Debug)]
pub struct BootstrapResult {
    pub peers: Vec<PeerRecord>,
    pub error: Option<String>,
}

// FGTW Seed Public Keys (hardcoded to avoid extra queries) X25519 public key - for encrypting announce messages
pub const FGTW_X25519_PUBLIC_KEY: [u8; 32] = [
    0x3D, 0x55, 0x63, 0xA3, 0x9C, 0xB4, 0x0F, 0x68, 0x0E, 0x20, 0x88, 0x76, 0xDC, 0x2E, 0x3E, 0x58,
    0xC2, 0xFB, 0xF4, 0xA0, 0x37, 0x60, 0xB1, 0x25, 0x61, 0xC0, 0xAF, 0xE1, 0x12, 0xAD, 0xDD, 0x11,
];

// Ed25519 public key - for verifying challenge signatures
pub const FGTW_ED25519_PUBLIC_KEY: [u8; 32] = [
    0x6D, 0x9F, 0x6E, 0x73, 0xBF, 0xA4, 0x83, 0x11, 0x58, 0x63, 0x42, 0x7C, 0xC7, 0x50, 0x5D, 0xC4,
    0x8F, 0xA7, 0x01, 0x6A, 0x60, 0xA6, 0xF4, 0x02, 0x05, 0xCA, 0x95, 0x0D, 0x9B, 0xF0, 0x58, 0x88,
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

/// Turn a worker `error`-frame `(reason, detail)` into a short user-facing message. The worker now
/// answers every failure this way at HTTP 200; the `detail` string is already plain (no web-stack
/// jargon), so surface it verbatim, keeping the operation `step` for context.
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
    // The fleet client uses the blocking HTTP path, so bridge through spawn_blocking rather than calling it from this async context.
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

    // App-level failure first: the worker answers every failure with a VSF `error` frame at HTTP 200
    // (`not_fleet_member`, `bad_signature`, …). Fall back to the legacy VSF-error / transport phrasing
    // only if it isn't one of the new reason frames.
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

    // Parse peer list
    let peers = parse_peer_list(&response_bytes, device_key)?;

    crate::log(&format!("FGTW: Received {} peer(s)", peers.len()));

    Ok(peers)
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

/// Convert Ed25519 secret key to X25519 secret key (RFC 8032) This is a one-way deterministic conversion using SHA-512 and clamping Matches FGTW's implementation for compatibility
fn ed25519_secret_to_x25519(ed25519_secret: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha512};

    // Hash the Ed25519 secret key
    let mut hasher = Sha512::new();
    hasher.update(ed25519_secret);
    let hash = hasher.finalize();

    // Take first 32 bytes and clamp them for X25519
    let mut x25519_secret = [0u8; 32];
    x25519_secret.copy_from_slice(&hash[..32]);

    // Clamp the secret key (RFC 7748)
    x25519_secret[0] &= 248;
    x25519_secret[31] &= 127;
    x25519_secret[31] |= 64;

    x25519_secret
}

/// Decrypt data from FGTW using ephemeral X25519 + AES-256-GCM Format: [ephemeral_pubkey:32][nonce:12][ciphertext+tag] The device_key is Ed25519 but we derive X25519 for decryption
fn decrypt_from_fgtw(
    ciphertext_with_header: &[u8],
    device_key: &Keypair,
) -> Result<Vec<u8>, String> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use x25519_dalek::{PublicKey, StaticSecret};

    if ciphertext_with_header.len() < 44 {
        // 32 (ephemeral pubkey) + 12 (nonce) = 44 minimum
        return Err("Ciphertext too short".to_string());
    }

    // Extract ephemeral public key (first 32 bytes)
    let mut ephemeral_pubkey_bytes = [0u8; 32];
    ephemeral_pubkey_bytes.copy_from_slice(&ciphertext_with_header[0..32]);
    let ephemeral_pubkey = PublicKey::from(ephemeral_pubkey_bytes);

    // Extract nonce (next 12 bytes)
    let mut nonce_bytes = [0u8; 12];
    nonce_bytes.copy_from_slice(&ciphertext_with_header[32..44]);
    let nonce = Nonce::from(nonce_bytes);

    // Remaining bytes are ciphertext+tag
    let ciphertext = &ciphertext_with_header[44..];

    // Convert Ed25519 secret key to X25519 secret key using RFC 8032 method This matches FGTW's conversion: SHA-512 hash + clamping
    let x25519_secret_bytes = ed25519_secret_to_x25519(device_key.secret.as_bytes());
    let x25519_secret = StaticSecret::from(x25519_secret_bytes);

    // Perform ECDH with ephemeral public key
    let shared_secret = x25519_secret.diffie_hellman(&ephemeral_pubkey);

    // Derive AES-256-GCM key from shared secret (32 bytes)
    let cipher = Aes256Gcm::new(shared_secret.as_bytes().into());

    // Decrypt
    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|e| format!("Decryption error: {}", e))?;

    Ok(plaintext)
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

/// Parse peer list from VSF bytes
fn parse_peer_list(bytes: &[u8], device_key: &Keypair) -> Result<Vec<PeerRecord>, String> {
    // 1+2. Verified whole-document read (hp + hb) + schema-validated section parse — the response cannot be read without verification passing first.
    let schema = vsf::schema::SectionSchema::new("encrypted_peers")
        .field("data", vsf::schema::TypeConstraint::Wrapped(b'e'));
    let section = vsf::schema::SectionBuilder::parse_document(schema, bytes, None)
        .map_err(|e| format!("Verified parse of encrypted_peers response: {}", e))?;

    // 3. Extract the v'e' encrypted blob from the "data" field (encoding enforced by the Wrapped(b'e') constraint above).
    let encrypted_data = section
        .get_value::<Vec<u8>>("data")
        .map_err(|e| format!("encrypted_peers data field: {}", e))?;

    // 5. Decrypt to get raw section data (just `[peers: ...]`, no VSF header)
    let plaintext_bytes = decrypt_from_fgtw(&encrypted_data, device_key)?;

    // Log decrypted section with inspector
    #[cfg(feature = "development")]
    {
        let msg = crate::network::inspect::section_inspect(
            &plaintext_bytes,
            "FGTW",
            "Decrypted",
            "peers",
        );
        if !msg.is_empty() {
            crate::log(&msg);
        }
    }

    // 6. Parse the peers section directly (no header, just `[peers: ...]`)
    let mut ptr = 0;
    let peers_section = VsfSection::parse(&plaintext_bytes, &mut ptr)
        .map_err(|e| format!("Parse peers section: {}", e))?;

    // 9. Get all peer fields and convert to PeerRecords.
    // Per-record skip: one malformed record must NOT abort the whole peer list — a single bad entry
    // used to `?`-bail here, leaving the requester with zero peers (so it never dialled anyone and
    // presence went one-way). Skip the bad record loudly and keep the rest.
    let peer_fields = peers_section.get_fields("peer");
    let mut peers = Vec::new();
    for (idx, field) in peer_fields.into_iter().enumerate() {
        match parse_peer_from_field(field) {
            Ok(peer) => peers.push(peer),
            Err(e) => crate::log(&format!(
                "Bootstrap: skipping malformed peer record at index {} = {}",
                idx, e
            )),
        }
    }

    Ok(peers)
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
