use super::{fingerprint::Keypair, PeerRecord};
use crate::types::DevicePubkey;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use vsf::{schema::FromVsfType, VsfHeader, VsfSection};

const FGTW_URL: &str = "https://fgtw.org";

/// Result of a bootstrap query - includes peers even on error
#[derive(Debug)]
pub struct BootstrapResult {
    pub peers: Vec<PeerRecord>,
    pub error: Option<String>,
}

// FGTW Seed Public Keys (hardcoded to avoid extra queries)
// X25519 public key - for encrypting announce messages
pub const FGTW_X25519_PUBLIC_KEY: [u8; 32] = [
    0x3D, 0x55, 0x63, 0xA3, 0x9C, 0xB4, 0x0F, 0x68, 0x0E, 0x20, 0x88, 0x76, 0xDC, 0x2E, 0x3E, 0x58,
    0xC2, 0xFB, 0xF4, 0xA0, 0x37, 0x60, 0xB1, 0x25, 0x61, 0xC0, 0xAF, 0xE1, 0x12, 0xAD, 0xDD, 0x11,
];

// Ed25519 public key - for verifying challenge signatures
pub const FGTW_ED25519_PUBLIC_KEY: [u8; 32] = [
    0x6D, 0x9F, 0x6E, 0x73, 0xBF, 0xA4, 0x83, 0x11, 0x58, 0x63, 0x42, 0x7C, 0xC7, 0x50, 0x5D, 0xC4,
    0x8F, 0xA7, 0x01, 0x6A, 0x60, 0xA6, 0xF4, 0x02, 0x05, 0xCA, 0x95, 0x0D, 0x9B, 0xF0, 0x58, 0x88,
];

/// Try to parse a VSF error message from response bytes
/// Returns Some(error_message) if the response is a valid VSF error, None otherwise
/// Uses VsfHeader::decode() for robust parsing
fn try_parse_vsf_error(bytes: &[u8]) -> Option<String> {
    use vsf::VsfType;

    // Use VsfHeader::decode() to parse the header
    let (header, header_len) = VsfHeader::decode(bytes).ok()?;

    // Look for "error" field in header fields
    for field in &header.fields {
        if field.name == "error" {
            // Try to parse the error section at the field's offset
            let mut ptr = field.offset_bytes;
            if let Ok(section) = VsfSection::parse(bytes, &mut ptr) {
                // Look for error message in section fields - try "message" first, then "error"
                for field_name in &["message", "error"] {
                    if let Some(section_field) = section.get_field(field_name) {
                        // Return first text value (l for long text, x for VSF text)
                        for value in &section_field.values {
                            match value {
                                VsfType::l(msg) => return Some(msg.clone()),
                                VsfType::x(msg) => return Some(msg.clone()),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback: check if there's an error section without header field
    // (for simple inline error responses)
    let mut ptr = header_len;
    while ptr < bytes.len() {
        if bytes[ptr] == b'[' {
            if let Ok(section) = VsfSection::parse(bytes, &mut ptr) {
                if section.name == "error" {
                    // Look for message field
                    for field_name in &["message", "error"] {
                        if let Some(section_field) = section.get_field(field_name) {
                            for value in &section_field.values {
                                match value {
                                    VsfType::l(msg) => return Some(msg.clone()),
                                    VsfType::x(msg) => return Some(msg.clone()),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        } else {
            break;
        }
    }

    None
}

/// Load bootstrap peers by announcing to FGTW
/// This requires authenticating with our handle and device key
/// Returns BootstrapResult which includes peers even on error (for peer discovery)
///
/// # Arguments
/// * `device_key` - Device's Ed25519 keypair
/// * `handle_proof` - Handle proof hash
/// * `port` - Local P2P port
/// * `handle` - User's handle string (for avatar keypair derivation)
pub async fn load_bootstrap_peers(
    device_key: &Keypair,
    handle_proof: [u8; 32],
    port: u16,
    handle: &str,
) -> BootstrapResult {
    match load_bootstrap_peers_inner(device_key, handle_proof, port, handle).await {
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
    handle: &str,
) -> Result<Vec<PeerRecord>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .pool_max_idle_per_host(0) // Disable connection pooling to avoid stale connections
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // Get challenge from FGTW (via conduit with empty challenge section)
    let challenge_request = vsf::vsf_builder::VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .provenance_only()
        .add_section("challenge", vec![])
        .build()
        .map_err(|e| format!("Build challenge request: {}", e))?;

    let challenge_url = format!("{}/conduit", FGTW_URL);

    #[cfg(feature = "development")]
    crate::log(&format!("FGTW: Sending challenge to {}", challenge_url));

    let challenge_response = client
        .post(&challenge_url)
        .header("Content-Type", "application/octet-stream")
        .body(challenge_request)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch challenge: {}", e))?;

    let challenge_status = challenge_response.status();

    #[cfg(feature = "development")]
    crate::log(&format!("FGTW: Challenge response status: {}", challenge_status));

    if !challenge_status.is_success() {
        return Err(format!(
            "Challenge HTTP error: {}",
            challenge_status
        ));
    }

    let challenge_bytes = challenge_response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read challenge: {}", e))?;

    #[cfg(feature = "development")]
    crate::log(&crate::network::inspect::vsf_inspect(
        &challenge_bytes,
        "FGTW",
        "RX",
        "conduit/challenge",
    ));

    // Parse challenge to extract provenance hash
    let challenge_hash = parse_challenge_hash(&challenge_bytes)?;

    // Derive avatar keypair for authentication
    let (_, avatar_verifying_key) =
        crate::avatar::derive_avatar_keypair(&device_key.secret, handle);
    let avatar_pub_key = Some(*avatar_verifying_key.as_bytes());

    // Build announce message with challenge response and avatar pubkey
    let announce_bytes = build_announce_message(
        handle_proof,
        device_key,
        port,
        challenge_hash,
        avatar_pub_key,
    )?;

    // Send announce to FGTW via conduit
    let announce_url = format!("{}/conduit", FGTW_URL);

    #[cfg(feature = "development")]
    crate::log(&format!("FGTW: Sending announce to {}", announce_url));

    let announce_response = client
        .post(&announce_url)
        .header("Content-Type", "application/octet-stream")
        .body(announce_bytes)
        .send()
        .await
        .map_err(|e| format!("Failed to send announce: {}", e))?;

    let status = announce_response.status();

    #[cfg(feature = "development")]
    crate::log(&format!("FGTW: Announce response status: {}", status));
    let is_success = status.is_success();

    let response_bytes = announce_response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    #[cfg(feature = "development")]
    crate::log(&crate::network::inspect::vsf_inspect(
        &response_bytes,
        "FGTW",
        "RX",
        "conduit/announce",
    ));

    if !is_success {
        if let Some(error_msg) = try_parse_vsf_error(&response_bytes) {
            return Err(error_msg);
        } else {
            return Err(format!("Announce HTTP error: {}", status));
        }
    }

    // Parse peer list
    let peers = parse_peer_list(&response_bytes, device_key)?;

    crate::log(&format!("FGTW: Received {} peer(s)", peers.len()));

    Ok(peers)
}

/// Parse challenge VSF to extract provenance hash
/// The timestamp in the challenge is ignored - announce generates its own timestamp
fn parse_challenge_hash(bytes: &[u8]) -> Result<[u8; 32], String> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    use vsf::VsfType;

    // Use VsfHeader::decode() to parse the entire header
    let (header, _header_len) = VsfHeader::decode(bytes)?;

    // Extract provenance hash (hp) - this is what gets signed
    let prov_hash_bytes = match &header.provenance_hash {
        VsfType::hp(hash) if hash.len() == 32 => hash.clone(),
        VsfType::hp(hash) => return Err(format!("Invalid provenance hash length: {}", hash.len())),
        _ => return Err("Invalid provenance hash type".to_string()),
    };

    // Extract signature (ge) - must be present for challenge
    let signature_bytes = match &header.signature {
        Some(VsfType::ge(sig)) if sig.len() == 64 => sig.clone(),
        Some(VsfType::ge(sig)) => {
            return Err(format!(
                "Invalid signature length: {} (expected 64)",
                sig.len()
            ))
        }
        _ => return Err("Challenge missing signature (ge)".to_string()),
    };

    // Verify signature over provenance hash
    let verifying_key = VerifyingKey::from_bytes(&FGTW_ED25519_PUBLIC_KEY)
        .map_err(|e| format!("Invalid FGTW public key: {}", e))?;

    let signature = Signature::from_bytes(
        signature_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "Invalid signature bytes".to_string())?,
    );

    verifying_key
        .verify(&prov_hash_bytes, &signature)
        .map_err(|_| "Challenge signature verification failed - not from authentic FGTW")?;

    // Return the provenance hash (which becomes the challenge value)
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&prov_hash_bytes);
    Ok(arr)
}

/// Encrypt data for FGTW using ephemeral X25519 + AES-256-GCM
/// Format: [ephemeral_pubkey:32][nonce:12][ciphertext+tag]
/// This matches FGTW's Web Crypto API implementation
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

/// Convert Ed25519 secret key to X25519 secret key (RFC 8032)
/// This is a one-way deterministic conversion using SHA-512 and clamping
/// Matches FGTW's implementation for compatibility
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

/// Decrypt data from FGTW using ephemeral X25519 + AES-256-GCM
/// Format: [ephemeral_pubkey:32][nonce:12][ciphertext+tag]
/// The device_key is Ed25519 but we derive X25519 for decryption
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

    // Convert Ed25519 secret key to X25519 secret key using RFC 8032 method
    // This matches FGTW's conversion: SHA-512 hash + clamping
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

/// Build VSF announce message (new encrypted format)
/// Structure: RÅ< z y b ef6 hp ke ge n[1] (d"announce" o b n) > [announce payload]
/// The device Ed25519 key (ke) and signature (ge) are at HEADER level for full file integrity
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
        .creation_time_nanos(vsf::eagle_time_nanos())
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
        &vsf_bytes,
        "FGTW",
        "TX",
        "/announce",
    ));

    Ok(vsf_bytes)
}

/// Parse peer list from VSF bytes
fn parse_peer_list(bytes: &[u8], device_key: &Keypair) -> Result<Vec<PeerRecord>, String> {
    // 1. Parse outer VSF file wrapper (proper VSF with header + provenance)
    let (outer_header, _) =
        VsfHeader::decode(bytes).map_err(|e| format!("Parse response header: {}", e))?;

    // 2. Find encrypted_peers section in outer wrapper
    let section_offset = outer_header
        .fields
        .iter()
        .find(|f| f.name == "encrypted_peers")
        .map(|f| f.offset_bytes)
        .ok_or("Missing 'encrypted_peers' section in response")?;

    // 3. Parse section to get encrypted data field
    let mut ptr = section_offset;
    let section = VsfSection::parse(bytes, &mut ptr)
        .map_err(|e| format!("Parse encrypted_peers section: {}", e))?;

    // 4. Extract v'e' encrypted blob from "data" field
    let encrypted_data = section
        .get_field("data")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            vsf::VsfType::v(b'e', data) => Some(data.clone()),
            _ => None,
        })
        .ok_or("Missing encrypted data field (v'e') in encrypted_peers section")?;

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

    // 9. Get all peer fields and convert to PeerRecords
    let peer_fields = peers_section.get_fields("peer");
    let mut peers = Vec::new();
    for field in peer_fields {
        let peer = parse_peer_from_field(field)?;
        peers.push(peer);
    }

    Ok(peers)
}

/// Parse a PeerRecord from a VsfField
/// Expected format: (peer: hb{32}, ke{32}, t_u3{IP}, u3{port}, ef6{timestamp})
fn parse_peer_from_field(field: &vsf::VsfField) -> Result<PeerRecord, String> {
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

    // Parse timestamp (e with EtType::f6)
    let last_seen = match &field.values[4] {
        vsf::VsfType::e(et) => match et {
            vsf::types::EtType::f6(timestamp) => *timestamp,
            _ => return Err("Expected f6 Eagle Time timestamp".to_string()),
        },
        _ => return Err("Expected Eagle Time (e) type for timestamp".to_string()),
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

    Ok(PeerRecord {
        handle_proof,
        device_pubkey,
        ip: SocketAddr::new(parsed_ip, port),
        local_ip,
        last_seen,
    })
}
