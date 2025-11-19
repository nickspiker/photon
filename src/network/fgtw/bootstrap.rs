use super::{storage::Keypair, PeerRecord};
use vsf::parse;

const FGTW_URL: &str = "https://fgtw.org";

// FGTW Seed Public Key (hardcoded to avoid extra queries)
// This is the X25519 public key of the fgtw.org bootstrap server
// Used for encrypting announce messages
pub const FGTW_X25519_PUBLIC_KEY: [u8; 32] = [
    0x3D, 0x55, 0x63, 0xA3, 0x9C, 0xB4, 0x0F, 0x68,
    0x0E, 0x20, 0x88, 0x76, 0xDC, 0x2E, 0x3E, 0x58,
    0xC2, 0xFB, 0xF4, 0xA0, 0x37, 0x60, 0xB1, 0x25,
    0x61, 0xC0, 0xAF, 0xE1, 0x12, 0xAD, 0xDD, 0x11,
];

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

    if !announce_response.status().is_success() {
        return Err(format!("Announce HTTP error: {}", announce_response.status()));
    }

    let peer_list_bytes = announce_response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read peer list: {}", e))?;

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
    // Use VSF's compute_provenance_hash to extract the hp field
    // This handles all the encoding details for us
    let hash_bytes = vsf::verification::compute_provenance_hash(bytes)
        .map_err(|e| format!("Failed to extract challenge hash: {}", e))?;

    eprintln!("DEBUG parse_challenge: Extracted challenge hash: {}", hex::encode(&hash_bytes));
    Ok(hash_bytes)
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

    // First check if this is an encrypted response (v'e')
    let plaintext_bytes = match parse(bytes, &mut ptr).map_err(|e| format!("Parse response type: {}", e))? {
        vsf::VsfType::v(b'e', encrypted_data) => {
            println!("Response is encrypted, decrypting...");
            // Decrypt the encrypted payload
            decrypt_from_fgtw(&encrypted_data, device_key)?
        }
        // If it's already a peer count, we have unencrypted data
        vsf::VsfType::u(v, _) => {
            println!("Response is unencrypted");
            // Reset ptr and return original bytes
            return parse_unencrypted_peer_list(bytes, v);
        }
        vsf::VsfType::u3(v) => {
            println!("Response is unencrypted");
            return parse_unencrypted_peer_list(bytes, v as usize);
        }
        vsf::VsfType::u4(v) => {
            println!("Response is unencrypted");
            return parse_unencrypted_peer_list(bytes, v as usize);
        }
        other => return Err(format!("Invalid peer list type: {:?}", other)),
    };

    // Parse the decrypted plaintext
    ptr = 0;
    let count = match parse(&plaintext_bytes, &mut ptr).map_err(|e| format!("Parse peer count: {}", e))? {
        vsf::VsfType::u(v, _) => v,
        vsf::VsfType::u3(v) => v as usize,
        vsf::VsfType::u4(v) => v as usize,
        _ => return Err("Invalid peer count type".to_string()),
    };

    println!("Received {} peer(s):", count);
    println!();

    let mut peers = Vec::with_capacity(count);
    for i in 0..count {
        let peer = parse_peer_record(&plaintext_bytes, &mut ptr)?;
        println!("  Peer {}: {}", i + 1, peer.ip);
        println!("    Handle Hash: {}", hex::encode(&peer.handle_hash[..8]));
        println!("    Device Key:  {}...", hex::encode(&peer.device_pubkey.as_bytes()[..8]));
        println!("    Last Seen:   {}", format_timestamp(peer.last_seen));
        println!();
        peers.push(peer);
    }

    Ok(peers)
}

/// Helper function to parse unencrypted peer list (for backward compatibility)
fn parse_unencrypted_peer_list(bytes: &[u8], count: usize) -> Result<Vec<PeerRecord>, String> {
    let mut ptr = 0;

    // Re-parse to position ptr after the count
    let _ = parse(bytes, &mut ptr).map_err(|e| format!("Parse peer count: {}", e))?;

    println!("Received {} peer(s):", count);
    println!();

    let mut peers = Vec::with_capacity(count);
    for i in 0..count {
        let peer = parse_peer_record(bytes, &mut ptr)?;
        println!("  Peer {}: {}", i + 1, peer.ip);
        println!("    Handle Hash: {}", hex::encode(&peer.handle_hash[..8]));
        println!("    Device Key:  {}...", hex::encode(&peer.device_pubkey.as_bytes()[..8]));
        println!("    Last Seen:   {}", format_timestamp(peer.last_seen));
        println!();
        peers.push(peer);
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

/// Parse a single peer record from VSF bytes
fn parse_peer_record(bytes: &[u8], ptr: &mut usize) -> Result<PeerRecord, String> {
    use crate::types::PublicIdentity;

    // Parse handle_hash (BLAKE3 hash)
    let hash_bytes = match parse(bytes, ptr).map_err(|e| format!("Parse handle_hash: {}", e))? {
        vsf::VsfType::hb(bytes) => bytes,
        _ => return Err("Invalid handle_hash type".to_string()),
    };
    println!("DEBUG parse_peer_record: handle_hash len={}, bytes={:02x?}", hash_bytes.len(), &hash_bytes[..hash_bytes.len().min(16)]);
    let mut handle_hash = [0u8; 32];
    handle_hash.copy_from_slice(&hash_bytes);

    // Parse device_pubkey (Ed25519 key from FGTW)
    let pubkey_bytes = match parse(bytes, ptr).map_err(|e| format!("Parse device_pubkey: {}", e))? {
        vsf::VsfType::ke(bytes) => bytes,
        _ => return Err("Invalid device_pubkey type".to_string()),
    };
    let mut pubkey_arr = [0u8; 32];
    pubkey_arr.copy_from_slice(&pubkey_bytes);
    let device_pubkey = PublicIdentity::from_bytes(pubkey_arr);

    // Parse IP as 1D tensor of u3 bytes (4 for IPv4, 16 for IPv6)
    let ip_tensor = match parse(bytes, ptr).map_err(|e| format!("Parse ip tensor: {}", e))? {
        vsf::VsfType::t_u3(tensor) => tensor,
        _ => return Err("Invalid ip type (expected t_u3 tensor)".to_string()),
    };
    let ip_bytes = ip_tensor.data;
    let ip_addr = if ip_bytes.len() == 4 {
        // IPv4
        use std::net::{IpAddr, Ipv4Addr};
        let ipv4 = Ipv4Addr::new(ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]);
        IpAddr::V4(ipv4)
    } else if ip_bytes.len() == 16 {
        // IPv6
        use std::net::{IpAddr, Ipv6Addr};
        let mut octets = [0u8; 16];
        octets.copy_from_slice(&ip_bytes);
        IpAddr::V6(Ipv6Addr::from(octets))
    } else {
        return Err(format!("Invalid IP length: {} bytes", ip_bytes.len()));
    };

    // Parse port
    let port = match parse(bytes, ptr).map_err(|e| format!("Parse port: {}", e))? {
        vsf::VsfType::u(v, _) => v as u16,
        vsf::VsfType::u3(v) => v as u16,
        vsf::VsfType::u4(v) => v as u16,
        _ => return Err("Invalid port type".to_string()),
    };

    // Combine IP and port into SocketAddr
    let ip = std::net::SocketAddr::new(ip_addr, port);

    // Parse last_seen
    let last_seen = match parse(bytes, ptr).map_err(|e| format!("Parse last_seen: {}", e))? {
        vsf::VsfType::f6(v) => v,
        _ => return Err("Invalid last_seen type".to_string()),
    };

    Ok(PeerRecord {
        handle_hash,
        device_pubkey,
        ip,
        last_seen,
    })
}
