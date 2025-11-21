use super::{storage::Keypair, PeerRecord};
use vsf::{parse, schema::FromVsfType};

const FGTW_URL: &str = "https://fgtw.org";

// FGTW Seed Public Keys (hardcoded to avoid extra queries)
// X25519 public key - for encrypting announce messages
pub const FGTW_X25519_PUBLIC_KEY: [u8; 32] = [
    0x3D, 0x55, 0x63, 0xA3, 0x9C, 0xB4, 0x0F, 0x68,
    0x0E, 0x20, 0x88, 0x76, 0xDC, 0x2E, 0x3E, 0x58,
    0xC2, 0xFB, 0xF4, 0xA0, 0x37, 0x60, 0xB1, 0x25,
    0x61, 0xC0, 0xAF, 0xE1, 0x12, 0xAD, 0xDD, 0x11,
];

// Ed25519 public key - for verifying challenge signatures
pub const FGTW_ED25519_PUBLIC_KEY: [u8; 32] = [
    0x6D, 0x9F, 0x6E, 0x73, 0xBF, 0xA4, 0x83, 0x11,
    0x58, 0x63, 0x42, 0x7C, 0xC7, 0x50, 0x5D, 0xC4,
    0x8F, 0xA7, 0x01, 0x6A, 0x60, 0xA6, 0xF4, 0x02,
    0x05, 0xCA, 0x95, 0x0D, 0x9B, 0xF0, 0x58, 0x88,
];

/// Try to parse a VSF error message from response bytes
/// Returns Some(error_message) if the response is a valid VSF error, None otherwise
/// Uses VSF crate's built-in field parser for robust parsing
fn try_parse_vsf_error(bytes: &[u8]) -> Option<String> {
    use vsf::file_format::VsfField;
    use vsf::VsfType;

    // Check VSF magic header "RÅ<"
    let magic = "RÅ<".as_bytes();
    if bytes.len() < magic.len() || &bytes[0..magic.len()] != magic {
        return None;
    }

    let mut ptr = magic.len(); // Skip "RÅ<"

    // Skip version, compat, header_length, timestamp
    for _ in 0..4 {
        if parse(bytes, &mut ptr).is_err() {
            return None;
        }
    }

    // Skip provenance hash (hp) or signature (ge)
    if parse(bytes, &mut ptr).is_err() {
        return None;
    }

    // Skip optional rolling hash (hb) if present
    if ptr < bytes.len() && bytes[ptr] == b'h' {
        if parse(bytes, &mut ptr).is_err() {
            return None;
        }
    }

    // Skip optional signature (ge) if present
    if ptr < bytes.len() && bytes[ptr] == b'g' {
        if parse(bytes, &mut ptr).is_err() {
            return None;
        }
    }

    // Parse field count
    let field_count = match parse(bytes, &mut ptr) {
        Ok(VsfType::n(count)) => count,
        _ => return None,
    };

    // Parse fields using VsfField::parse() from VSF crate
    for _ in 0..field_count {
        let field = VsfField::parse(bytes, &mut ptr).ok()?;

        // Look for "error" field
        if field.name == "error" {
            // Extract error message from first value (should be VsfType::l)
            if let Some(VsfType::l(error_msg)) = field.values.first() {
                return Some(error_msg.clone());
            }
        }
    }

    None
}

/// Load bootstrap peers by announcing to FGTW
/// This requires authenticating with our handle and device key
pub async fn load_bootstrap_peers(
    device_key: &Keypair,
    handle_hash: [u8; 32],
    port: u16,
) -> Result<Vec<PeerRecord>, String> {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║ FGTW Bootstrap Authentication                              ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    println!("Server: {}", FGTW_URL);
    println!("Handle Hash: {}", hex::encode(&handle_hash));
    println!("Port: {}", port);
    println!();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // ═══ Step 1: Get challenge from FGTW ═══
    println!("─────────────────────────────────────────────────────────────");
    println!("Step 1: Request Challenge");
    println!("─────────────────────────────────────────────────────────────");
    println!("GET {}/challenge", FGTW_URL);

    let challenge_response = client
        .get(&format!("{}/challenge", FGTW_URL))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch challenge: {}", e))?;

    if !challenge_response.status().is_success() {
        return Err(format!("Challenge HTTP error: {}", challenge_response.status()));
    }

    let challenge_bytes = challenge_response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read challenge: {}", e))?;

    println!("Response: {} bytes", challenge_bytes.len());

    // DEBUG: Save challenge for inspection
    std::fs::write("/tmp/photon-challenge.vsf", &challenge_bytes).ok();
    println!("Saved to: /tmp/photon-challenge.vsf");

    // ═══ Step 2: Parse challenge to extract provenance hash ═══
    println!();
    println!("─────────────────────────────────────────────────────────────");
    println!("Step 2: Parse Challenge");
    println!("─────────────────────────────────────────────────────────────");

    let challenge_hash = parse_challenge_hash(&challenge_bytes)?;

    println!("Challenge Hash (hp): {}", hex::encode(&challenge_hash));

    // ═══ Step 3: Build announce message with challenge provenance hash ═══
    println!();
    println!("─────────────────────────────────────────────────────────────");
    println!("Step 3: Build Announce Message");
    println!("─────────────────────────────────────────────────────────────");

    let announce_bytes = build_announce_message(handle_hash, device_key, port, challenge_hash)?;

    println!("Built VSF announce: {} bytes", announce_bytes.len());
    println!("  • Encrypted with FGTW X25519 key");
    println!("  • Signed with device Ed25519 key");
    println!("  • Includes challenge response");

    // DEBUG: Save announce message for inspection
    std::fs::write("/tmp/photon-announce.vsf", &announce_bytes).ok();
    println!("Saved to: /tmp/photon-announce.vsf");

    // ═══ Step 4: Send announce to FGTW ═══
    println!();
    println!("─────────────────────────────────────────────────────────────");
    println!("Step 4: Send Announce");
    println!("─────────────────────────────────────────────────────────────");
    println!("POST {}/announce", FGTW_URL);

    let announce_response = client
        .post(&format!("{}/announce", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(announce_bytes)
        .send()
        .await
        .map_err(|e| format!("Failed to send announce: {}", e))?;

    // Capture status before consuming response
    let status = announce_response.status();
    let is_success = status.is_success();

    // Read response body
    let response_bytes = announce_response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    if !is_success {
        // Try to parse VSF error message from response body
        eprintln!("DEBUG: Got error response, {} bytes, first 32 bytes: {:?}",
                  response_bytes.len(),
                  &response_bytes[..response_bytes.len().min(32)]);

        if let Some(error_msg) = try_parse_vsf_error(&response_bytes) {
            return Err(format!("FGTW error: {}", error_msg));
        } else {
            eprintln!("DEBUG: Failed to parse VSF error message");
            return Err(format!("Announce HTTP error: {}", status));
        }
    }

    let peer_list_bytes = response_bytes;

    println!("Response: {} bytes", peer_list_bytes.len());

    // ═══ Step 5: Parse peer list ═══
    println!();
    println!("─────────────────────────────────────────────────────────────");
    println!("Step 5: Parse Peer List");
    println!("─────────────────────────────────────────────────────────────");

    let peers = parse_peer_list(&peer_list_bytes, device_key)?;

    println!();
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║ Bootstrap Complete: {} peer(s)                           ║", peers.len());
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();

    Ok(peers)
}

/// Parse challenge VSF to extract provenance hash
/// The timestamp in the challenge is ignored - announce generates its own timestamp
fn parse_challenge_hash(bytes: &[u8]) -> Result<[u8; 32], String> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    // Parse VSF to extract signature (ge) from header
    let mut ptr = 4; // Skip "RÅ<"

    // Skip version, backward_compat, header_length, timestamp
    let _ = parse(bytes, &mut ptr).map_err(|e| format!("Parse version: {}", e))?;
    let _ = parse(bytes, &mut ptr).map_err(|e| format!("Parse backward compat: {}", e))?;
    let _ = parse(bytes, &mut ptr).map_err(|e| format!("Parse header length: {}", e))?;
    let _ = parse(bytes, &mut ptr).map_err(|e| format!("Parse timestamp: {}", e))?;

    // Extract provenance hash (hp) - this is what gets signed
    let prov_hash_result = parse(bytes, &mut ptr).map_err(|e| format!("Parse provenance hash: {}", e))?;
    let prov_hash_bytes = match &prov_hash_result {
        vsf::VsfType::hp(hash) => {
            if hash.len() != 32 {
                return Err(format!("Invalid provenance hash length: {}", hash.len()));
            }
            hash.clone()
        }
        _ => return Err("Invalid provenance hash type".to_string()),
    };

    // Skip rolling hash if present
    let next_type = parse(bytes, &mut ptr).map_err(|e| format!("Parse after hp: {}", e))?;

    // Check if next is signature (ge) or rolling hash (hb)
    let signature_bytes = match &next_type {
        vsf::VsfType::ge(sig) => sig.clone(),
        vsf::VsfType::hb(_) => {
            // Skip rolling hash, next should be signature
            let sig_type = parse(bytes, &mut ptr).map_err(|e| format!("Parse signature: {}", e))?;
            match sig_type {
                vsf::VsfType::ge(sig) => sig,
                _ => return Err("Challenge missing signature (ge)".to_string()),
            }
        }
        _ => return Err("Challenge missing signature (ge)".to_string()),
    };

    if signature_bytes.len() != 64 {
        return Err(format!("Invalid signature length: {} (expected 64)", signature_bytes.len()));
    }

    // Verify signature over provenance hash
    let verifying_key = VerifyingKey::from_bytes(&FGTW_ED25519_PUBLIC_KEY)
        .map_err(|e| format!("Invalid FGTW public key: {}", e))?;

    let signature = Signature::from_bytes(signature_bytes.as_slice().try_into()
        .map_err(|_| "Invalid signature bytes".to_string())?);

    verifying_key.verify(&prov_hash_bytes, &signature)
        .map_err(|_| "Challenge signature verification failed - not from authentic FGTW")?;

    println!("✓ Challenge signature verified (authentic FGTW response)");

    // Return the provenance hash (which becomes the challenge value)
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&prov_hash_bytes);
    eprintln!("DEBUG parse_challenge: Extracted challenge hash: {}", hex::encode(&arr));
    Ok(arr)
}

/// Encrypt data for FGTW using ephemeral X25519 + AES-256-GCM
/// Format: [ephemeral_pubkey:32][nonce:12][ciphertext+tag]
/// This matches FGTW's Web Crypto API implementation
fn encrypt_for_fgtw(plaintext: &[u8], fgtw_x25519_pubkey: &[u8; 32]) -> Result<Vec<u8>, String> {
    use aes_gcm::{
        aead::{Aead, KeyInit, OsRng},
        Aes256Gcm, Nonce,
    };
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
fn decrypt_from_fgtw(ciphertext_with_header: &[u8], device_key: &Keypair) -> Result<Vec<u8>, String> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use ed25519_dalek::SecretKey;
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
/// Structure: RÅ< z y b ef6 hp n[1] (d"announce" ke v'e'[encrypted] o b n0) > [d"announce" v(b'e', encrypted[hb(challenge) + hb(handle) + u(port)])]
/// The device Ed25519 key (ke) is in the header field, signature (ge) is added by sign_section()
/// Timestamp is generated at flattening time by sign_section()
fn build_announce_message(
    handle_hash: [u8; 32],
    device_key: &Keypair,
    port: u16,
    challenge_hash: [u8; 32],
) -> Result<Vec<u8>, String> {
    use vsf::file_format::{HeaderField, VsfSection};
    use vsf::types::EtType;
    use vsf::verification::sign_section;
    use vsf::vsf_builder::VsfBuilder;
    use vsf::{VsfType, VSF_BACKWARD_COMPAT, VSF_VERSION};

    // 1. Build encrypted payload: hb(challenge_hash) + hb(handle_hash) + u(port)
    let mut plaintext = Vec::new();
    plaintext.extend(VsfType::hb(challenge_hash.to_vec()).flatten());
    plaintext.extend(VsfType::hb(handle_hash.to_vec()).flatten());
    plaintext.extend(VsfType::u(port as usize, false).flatten());

    // 2. Encrypt for FGTW using ephemeral X25519 + AES-GCM
    let encrypted = encrypt_for_fgtw(&plaintext, &FGTW_X25519_PUBLIC_KEY)?;

    // 3. Build the "announce" section with encrypted wrapper
    let mut section_bytes = Vec::new();
    section_bytes.push(b'[');
    section_bytes.extend(VsfType::d("announce".to_string()).flatten());
    section_bytes.extend(VsfType::v(b'e', encrypted).flatten());
    section_bytes.push(b']');

    // 4. Create VsfHeader without timestamp - sign_section() will generate one at flattening time
    let mut header = vsf::file_format::VsfHeader::new(VSF_VERSION, VSF_BACKWARD_COMPAT);

    // 5. Create header field with device Ed25519 key and encryption metadata
    // FGTW will derive X25519 from Ed25519 when needed for encryption
    let announce_field = HeaderField {
        name: "announce".to_string(),
        hash: None,
        signature: None, // Will be added by sign_section()
        key: Some(VsfType::ke(device_key.public.to_bytes().to_vec())), // Ed25519 device public key
        wrap: Some(VsfType::v(b'e', vec![])), // Empty vec = metadata only
        offset_bytes: 0, // Placeholder, will be fixed by stabilization
        size_bytes: section_bytes.len(),
        child_count: 0, // Encrypted sections have implied n[0]
    };
    header.add_field(announce_field);

    // 6. Use VsfHeader's encode and stabilization
    let mut header_bytes = header.encode()?;
    vsf::file_format::VsfHeader::update_header_length(&mut header_bytes)?;

    // Append section
    header_bytes.extend(&section_bytes);

    // 7. Sign the "announce" section using sign_section
    // This will parse the VSF, rebuild it with correct offsets, compute hashes, and add signature
    eprintln!("DEBUG build_announce: About to call sign_section, header_bytes len: {}", header_bytes.len());
    let signed_message = sign_section(header_bytes, "announce", device_key.secret.as_bytes())?;
    eprintln!("DEBUG build_announce: sign_section returned, signed_message len: {}", signed_message.len());

    // Check signature bytes in the result
    if signed_message.len() > 0xA9 {
        let sig_start = 0x6A;
        let sig_bytes = &signed_message[sig_start..sig_start+8];
        eprintln!("DEBUG build_announce: Signature bytes at 0x{:X}: {:02X?}", sig_start, sig_bytes);
    }

    Ok(signed_message)
}

/// Parse peer list from VSF bytes
fn parse_peer_list(bytes: &[u8], device_key: &Keypair) -> Result<Vec<PeerRecord>, String> {
    let mut ptr = 0;

    // Response MUST be encrypted (v'e') - authentication required
    let first_type = parse(bytes, &mut ptr).map_err(|e| format!("Parse response type: {}", e))?;
    let plaintext_bytes = match &first_type {
        vsf::VsfType::v(b'e', encrypted_data) => {
            println!("Response is encrypted, decrypting...");
            decrypt_from_fgtw(encrypted_data, device_key)?
        }
        _ => {
            return Err(format!(
                "Invalid peer list: must be encrypted (v'e') for authentication, got {:?}",
                first_type
            ));
        }
    };

    // The decrypted plaintext is now a complete VSF file with proper sections
    // Format: RÅ<[header]>[d"peer0" (d"handle_hash":hb) ...][d"peer1" ...]
    println!("Parsing decrypted VSF file ({} bytes)", plaintext_bytes.len());

    // Validate VSF magic number "RÅ" (3 bytes UTF-8) and '<'
    if plaintext_bytes.len() < 4 || &plaintext_bytes[0..3] != "RÅ".as_bytes() || plaintext_bytes[3] != b'<' {
        return Err("Invalid VSF format: missing magic bytes RÅ<".to_string());
    }

    // Parse VSF header to find peer section offsets
    ptr = 4; // Skip "RÅ<"

    // Skip version, backward_compat, header_length, creation_time
    let _ = parse(&plaintext_bytes, &mut ptr).map_err(|e| format!("Parse version: {}", e))?;
    let _ = parse(&plaintext_bytes, &mut ptr).map_err(|e| format!("Parse backward_compat: {}", e))?;
    let _ = parse(&plaintext_bytes, &mut ptr).map_err(|e| format!("Parse header_length: {}", e))?;
    let _ = parse(&plaintext_bytes, &mut ptr).map_err(|e| format!("Parse creation_time: {}", e))?;

    // Parse provenance hash (hp)
    let _ = parse(&plaintext_bytes, &mut ptr).map_err(|e| format!("Parse provenance hash: {}", e))?;

    // Skip optional rolling hash (hb) if present
    let next_type = parse(&plaintext_bytes, &mut ptr).map_err(|e| format!("Parse after hp: {}", e))?;
    let field_count = match next_type {
        vsf::VsfType::hb(_) => {
            match parse(&plaintext_bytes, &mut ptr).map_err(|e| format!("Parse field count: {}", e))? {
                vsf::VsfType::n(count) => count,
                _ => return Err("Invalid field count type".to_string()),
            }
        }
        vsf::VsfType::n(count) => count,
        _ => return Err("Expected field count or rolling hash".to_string()),
    };

    // Parse header field definitions to find section offsets
    let mut section_offsets = Vec::new();
    for _ in 0..field_count {
        if plaintext_bytes[ptr] != b'(' {
            return Err("Expected '(' for field".to_string());
        }
        ptr += 1;

        // Parse section name (d type)
        let section_name = match parse(&plaintext_bytes, &mut ptr).map_err(|e| format!("Parse section name: {}", e))? {
            vsf::VsfType::d(name) => name,
            _ => return Err("Invalid section name type".to_string()),
        };

        // Skip ':' separator
        if plaintext_bytes[ptr] != b':' {
            return Err("Expected ':' after section name".to_string());
        }
        ptr += 1;

        // Parse offset, size, child_count
        let mut offset = None;
        while ptr < plaintext_bytes.len() && plaintext_bytes[ptr] != b')' {
            let field = parse(&plaintext_bytes, &mut ptr).map_err(|e| format!("Parse header field: {}", e))?;
            match field {
                vsf::VsfType::o(o) => offset = Some(o),
                _ => {}
            }
            if ptr < plaintext_bytes.len() && plaintext_bytes[ptr] == b',' {
                ptr += 1;
            }
        }

        if plaintext_bytes[ptr] != b')' {
            return Err("Expected ')' after field".to_string());
        }
        ptr += 1;

        if let Some(off) = offset {
            section_offsets.push((section_name, off));
        }
    }

    // Skip '>'
    if plaintext_bytes[ptr] != b'>' {
        return Err("Expected '>' for header end".to_string());
    }
    ptr += 1;

    // Find the "peers" section
    let peers_offset = section_offsets.iter()
        .find(|(name, _)| name == "peers")
        .map(|(_, offset)| *offset)
        .ok_or("Missing 'peers' section")?;

    // Parse the peers section
    let mut peers = Vec::new();
    let mut ptr = peers_offset;

    // Expect section start '['
    if plaintext_bytes[ptr] != b'[' {
        return Err("Expected '[' for peers section start".to_string());
    }
    ptr += 1;

    // Parse section name "peers"
    let _ = parse(&plaintext_bytes, &mut ptr).map_err(|e| format!("Parse section name: {}", e))?;

    // Parse all (peer: ...) fields until ']'
    while ptr < plaintext_bytes.len() && plaintext_bytes[ptr] != b']' {
        // Skip whitespace
        while ptr < plaintext_bytes.len() && (plaintext_bytes[ptr] == b'\n' || plaintext_bytes[ptr] == b' ') {
            ptr += 1;
        }

        if plaintext_bytes[ptr] == b']' {
            break;
        }

        let (peer, new_ptr) = parse_peer_field(&plaintext_bytes, ptr)?;
        ptr = new_ptr;

        peers.push(peer);
    }

    println!("Received {} peer(s):", peers.len());
    println!();

    for (i, peer) in peers.iter().enumerate() {
        println!("  Peer {}: {}", i, peer.ip);
        println!("    Handle Hash: {}", hex::encode(&peer.handle_hash[..8]));
        println!("    Device Key:  {}...", hex::encode(&peer.device_pubkey.as_bytes()[..8]));
        println!("    Last Seen:   {}", format_timestamp(peer.last_seen));
        println!();
    }

    Ok(peers)
}

/// Format timestamp as human-readable
/// Timestamp is in Eagle Time, so we need to convert current time to Eagle Time for comparison
fn format_timestamp(eagle_ts: f64) -> String {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    // Get current Unix time
    let unix_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();

    // Convert to Eagle Time (add offset: Eagle epoch is 14,182,940 seconds before Unix epoch)
    let eagle_now = unix_now + 14182940.0;

    let diff = eagle_now - eagle_ts;

    if diff < 60.0 {
        format!("{:.0}s ago", diff)
    } else if diff < 3600.0 {
        format!("{:.0}m ago", diff / 60.0)
    } else if diff < 86400.0 {
        format!("{:.1}h ago", diff / 3600.0)
    } else {
        format!("{:.1}d ago", diff / 86400.0)
    }
}

/// Parse a single peer section from VSF bytes with named fields
fn parse_peer_field(bytes: &[u8], offset: usize) -> Result<(PeerRecord, usize), String> {
    use crate::types::PublicIdentity;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    let mut ptr = offset;

    // Expect field start '('
    if bytes[ptr] != b'(' {
        return Err("Expected '(' for peer field start".to_string());
    }
    ptr += 1;

    // Parse field name (should be "peer")
    let field_name = match parse(bytes, &mut ptr).map_err(|e| format!("Parse field name: {}", e))? {
        vsf::VsfType::d(name) => name,
        _ => return Err("Invalid field name type".to_string()),
    };

    if field_name != "peer" {
        return Err(format!("Expected field name 'peer', got '{}'", field_name));
    }

    // Skip ':' separator
    if bytes[ptr] != b':' {
        return Err("Expected ':' after field name".to_string());
    }
    ptr += 1;

    // Parse all comma-separated values for this peer
    // Expected format: (peer: hb{32}{...}, ke{32}{...}, t u3{IP}, u3{port}, ef6{timestamp})

    // Parse handle_hash (hb{32})
    let handle_hash = match parse(bytes, &mut ptr).map_err(|e| format!("Parse handle_hash: {}", e))? {
        vsf::VsfType::hb(h) if h.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&h);
            arr
        }
        _ => return Err("Invalid handle_hash type or length".to_string()),
    };

    // Expect comma separator
    if bytes[ptr] != b',' {
        return Err("Expected ',' after handle_hash".to_string());
    }
    ptr += 1;

    // Parse device_pubkey (ke{32})
    let device_pubkey = match parse(bytes, &mut ptr).map_err(|e| format!("Parse device_pubkey: {}", e))? {
        vsf::VsfType::ke(k) if k.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&k);
            PublicIdentity::from_bytes(arr)
        }
        _ => return Err("Invalid device_pubkey type or length".to_string()),
    };

    // Expect comma separator
    if bytes[ptr] != b',' {
        return Err("Expected ',' after device_pubkey".to_string());
    }
    ptr += 1;

    // Parse IP address (t u3{4 or 16 bytes})
    let ip_bytes = match parse(bytes, &mut ptr).map_err(|e| format!("Parse ip: {}", e))? {
        vsf::VsfType::t_u3(tensor) => tensor.data,
        _ => return Err("Invalid ip type".to_string()),
    };

    let parsed_ip = if ip_bytes.len() == 4 {
        IpAddr::V4(Ipv4Addr::new(ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]))
    } else if ip_bytes.len() == 16 {
        let mut octets = [0u8; 16];
        octets.copy_from_slice(&ip_bytes);
        IpAddr::V6(Ipv6Addr::from(octets))
    } else {
        return Err(format!("Invalid IP length: {}", ip_bytes.len()));
    };

    // Expect comma separator
    if bytes[ptr] != b',' {
        return Err("Expected ',' after ip".to_string());
    }
    ptr += 1;

    // Parse port (u3 or generic u)
    let port = u16::from_vsf_type(&parse(bytes, &mut ptr).map_err(|e| format!("Parse port: {}", e))?)
        .map_err(|e| format!("Invalid port: {}", e))?;

    // Expect comma separator
    if bytes[ptr] != b',' {
        return Err("Expected ',' after port".to_string());
    }
    ptr += 1;

    // Parse timestamp (e with EtType::f6)
    let last_seen = match parse(bytes, &mut ptr).map_err(|e| format!("Parse last_seen: {}", e))? {
        vsf::VsfType::e(et) => match et {
            vsf::types::EtType::f6(timestamp) => timestamp,
            _ => return Err("Expected f6 Eagle Time timestamp".to_string()),
        },
        _ => return Err("Expected Eagle Time (e) type for timestamp".to_string()),
    };

    // Expect field end ')'
    if bytes[ptr] != b')' {
        return Err("Expected ')' after peer field".to_string());
    }
    ptr += 1;

    Ok((PeerRecord {
        handle_hash,
        device_pubkey,
        ip: SocketAddr::new(parsed_ip, port),
        last_seen,
    }, ptr))
}
