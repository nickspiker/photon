use crate::types::DevicePubkey;
use std::net::{IpAddr, SocketAddr};
use vsf::schema::FromVsfType;
use vsf::types::Vector;
use vsf::VsfType;

/// FGTW protocol messages (VSF serialized)
#[derive(Debug, Clone)]
pub enum FgtwMessage {
    Ping {
        device_pubkey: DevicePubkey,
    },
    Pong {
        device_pubkey: DevicePubkey,
        peers: Vec<PeerRecord>,
    },
    FindNode {
        handle_proof: [u8; 32],
        requester_pubkey: DevicePubkey,
    },
    FoundNodes {
        devices: Vec<PeerRecord>,
    },
    Announce {
        handle_proof: [u8; 32],
        device_pubkey: DevicePubkey,
        port: u16,
    },
    Query {
        handle_proof: [u8; 32],
        requester_pubkey: DevicePubkey,
    },
    QueryResponse {
        devices: Vec<PeerRecord>,
    },
    /// P2P status ping - "are you online?"
    ///
    /// Simplified header-only format:
    /// RÅ< z4 y2 ef6[timestamp] hp[provenance] ke[pubkey] ge[signature] n1 (ping) >
    ///
    /// - provenance_hash = BLAKE3(sender_pubkey || timestamp_nanos)
    /// - ke = sender's Ed25519 public key (for signature verification)
    /// - ge = signature of provenance_hash
    ///
    /// Note: Avatar is fetched by handle, not exchanged in ping/pong.
    /// Storage key = BLAKE3(BLAKE3(handle) || "avatar")
    StatusPing {
        timestamp: f64, // Eagle time with nanosecond precision (ef6) but it's not an ef6?
        sender_pubkey: DevicePubkey, // Who is pinging (for response routing)
        provenance_hash: [u8; 32], // BLAKE3(sender_pubkey || timestamp_nanos)
        signature: [u8; 64], // Ed25519 signature of provenance_hash
    },
    /// P2P status pong - "yes I'm online"
    ///
    /// Simplified header-only format:
    /// RÅ< z4 y2 ef6[timestamp] hp[SAME provenance] ke[pubkey] ge[signature] n1 (pong) >
    ///
    /// - Echoes same provenance_hash from ping (proves we saw it)
    /// - ke = responder's Ed25519 public key (for signature verification)
    /// - ge = signature of provenance_hash (proves we processed it)
    ///
    /// Note: Avatar is fetched by handle, not exchanged in ping/pong.
    /// Storage key = BLAKE3(BLAKE3(handle) || "avatar")
    StatusPong {
        timestamp: f64,                 // Responder's current Eagle time (ef6)
        responder_pubkey: DevicePubkey, // Who is responding
        provenance_hash: [u8; 32],      // Same hash from ping (proves we received it)
        signature: [u8; 64],            // Ed25519 signature of provenance_hash
    },
    /// CLUTCH Offer - parallel key exchange (v2)
    ///
    /// Both parties send this simultaneously. Each party's ephemeral
    /// pubkey contributes entropy to the final seed.
    ///
    /// Format: section "clutch_offer" with handle proofs and ephemeral key
    /// - from_handle_proof: sender's handle proof
    /// - to_handle_proof: recipient's handle proof
    /// - ephemeral_x25519: sender's ephemeral public key for ECDH
    /// - signature: Ed25519 over provenance hash
    ClutchOffer {
        timestamp: f64,
        from_handle_proof: [u8; 32],
        to_handle_proof: [u8; 32],
        ephemeral_x25519: [u8; 32],
        sender_pubkey: DevicePubkey,
        signature: [u8; 64],
    },
    /// CLUTCH Init - initiator sends their ephemeral pubkey to start ceremony (v1 legacy)
    ///
    /// Format: header-only with clutch_init section name
    /// - from_handle_proof: initiator's handle proof
    /// - to_handle_proof: responder's handle proof
    /// - ephemeral_x25519: ephemeral public key for ECDH
    /// - signature: Ed25519 over (from || to || ephemeral)
    ClutchInit {
        timestamp: f64,
        from_handle_proof: [u8; 32],
        to_handle_proof: [u8; 32],
        ephemeral_x25519: [u8; 32],
        sender_pubkey: DevicePubkey, // For signature verification
        signature: [u8; 64],
    },
    /// CLUTCH Response - responder sends their ephemeral pubkey back (v1 legacy)
    ClutchResponse {
        timestamp: f64,
        from_handle_proof: [u8; 32],
        to_handle_proof: [u8; 32],
        ephemeral_x25519: [u8; 32],
        sender_pubkey: DevicePubkey,
        signature: [u8; 64],
    },
    /// CLUTCH Complete - initiator confirms they derived the same seed
    ClutchComplete {
        timestamp: f64,
        from_handle_proof: [u8; 32],
        to_handle_proof: [u8; 32],
        proof: [u8; 32], // BLAKE3(shared_seed || "CLUTCH_v1_complete")
        sender_pubkey: DevicePubkey,
        signature: [u8; 64],
    },
    /// Encrypted chat message
    ///
    /// Format: section "msg" with encrypted payload
    /// - from_handle_proof: sender's handle proof (for recipient to identify sender)
    /// - sequence: message sequence number
    /// - ciphertext: ChaCha20-Poly1305 encrypted message
    ChatMessage {
        timestamp: f64,
        from_handle_proof: [u8; 32],
        sequence: u64,
        ciphertext: Vec<u8>,
        sender_pubkey: DevicePubkey,
        signature: [u8; 64],
    },
    /// Message acknowledgment
    ///
    /// Confirms receipt of a message by sequence number.
    /// Includes two hashes for bidirectional chain weaving:
    /// - plaintext_hash: proves decryption of their message
    /// - sender_last_acked: our most recent msg they ACK'd (weaves our chain into theirs)
    MessageAck {
        timestamp: f64,
        from_handle_proof: [u8; 32],
        sequence: u64,
        /// BLAKE3 hash of decrypted plaintext - proves we decrypted correctly
        plaintext_hash: [u8; 32],
        /// Hash of our most recent message they ACK'd - bidirectional weave binding
        sender_last_acked: [u8; 32],
        sender_pubkey: DevicePubkey,
        signature: [u8; 64],
    },
}

/// Peer record - one device for a user handle
#[derive(Debug, Clone)]
pub struct PeerRecord {
    pub handle_proof: [u8; 32], // Memory-hard PoW output (24MB, 17 rounds)
    pub device_pubkey: DevicePubkey, // Device's X25519 public key (used as device identifier)
    pub ip: SocketAddr,         // Where to reach this device
    pub last_seen: f64,         // Timestamp (f64, serializes as VSF type f6)
}

/// Convert SocketAddr to binary format for VSF
/// Format:
/// - IPv4: 4 bytes (address) + 2 bytes (port big-endian) = 6 bytes
/// - IPv6: 16 bytes (address) + 2 bytes (port big-endian) = 18 bytes
fn socketaddr_to_bytes(addr: &SocketAddr) -> Vec<u8> {
    let mut bytes = Vec::new();

    // Add IP address bytes
    match addr.ip() {
        IpAddr::V4(ipv4) => {
            bytes.extend_from_slice(&ipv4.octets());
        }
        IpAddr::V6(ipv6) => {
            bytes.extend_from_slice(&ipv6.octets());
        }
    }

    // Add port (big-endian u16)
    bytes.extend_from_slice(&addr.port().to_be_bytes());

    bytes
}

/// Convert binary format back to SocketAddr
/// Returns None if the format is invalid
fn bytes_to_socketaddr(bytes: &[u8]) -> Option<SocketAddr> {
    if bytes.len() == 6 {
        // IPv4: 4 bytes address + 2 bytes port
        let ip = IpAddr::V4(std::net::Ipv4Addr::new(
            bytes[0], bytes[1], bytes[2], bytes[3],
        ));
        let port = u16::from_be_bytes([bytes[4], bytes[5]]);
        Some(SocketAddr::new(ip, port))
    } else if bytes.len() == 18 {
        // IPv6: 16 bytes address + 2 bytes port
        let mut octets = [0u8; 16];
        octets.copy_from_slice(&bytes[0..16]);
        let ip = IpAddr::V6(std::net::Ipv6Addr::from(octets));
        let port = u16::from_be_bytes([bytes[16], bytes[17]]);
        Some(SocketAddr::new(ip, port))
    } else {
        None
    }
}

impl FgtwMessage {
    /// Serialize to proper VSF file
    pub fn to_vsf_bytes(&self) -> Vec<u8> {
        use vsf::VsfBuilder;

        let builder = VsfBuilder::new();

        let result = match self {
            FgtwMessage::Ping { device_pubkey } => builder
                .add_section(
                    "fgtw",
                    vec![
                        ("msg_type".to_string(), VsfType::u3(0)),
                        ("device_pubkey".to_string(), device_pubkey.to_vsf()),
                    ],
                )
                .build(),
            FgtwMessage::Pong {
                device_pubkey,
                peers,
            } => {
                let mut fields = vec![
                    ("msg_type".to_string(), VsfType::u3(1)),
                    ("device_pubkey".to_string(), device_pubkey.to_vsf()),
                    ("peer_count".to_string(), VsfType::u(peers.len(), false)),
                ];

                // Add each peer as separate fields
                for (i, peer) in peers.iter().enumerate() {
                    let prefix = format!("peer_{}", i);
                    fields.push((
                        format!("{}_handle_proof", prefix),
                        VsfType::hb(peer.handle_proof.to_vec()),
                    ));
                    fields.push((
                        format!("{}_device_pubkey", prefix),
                        peer.device_pubkey.to_vsf(),
                    ));
                    fields.push((
                        format!("{}_ip", prefix),
                        VsfType::v_u3(Vector {
                            data: socketaddr_to_bytes(&peer.ip),
                        }),
                    ));
                    fields.push((format!("{}_last_seen", prefix), VsfType::f6(peer.last_seen)));
                }

                builder.add_section("fgtw", fields).build()
            }
            FgtwMessage::FindNode {
                handle_proof,
                requester_pubkey,
            } => builder
                .add_section(
                    "fgtw",
                    vec![
                        ("msg_type".to_string(), VsfType::u3(2)),
                        (
                            "handle_proof".to_string(),
                            VsfType::hb(handle_proof.to_vec()),
                        ),
                        ("requester_pubkey".to_string(), requester_pubkey.to_vsf()),
                    ],
                )
                .build(),
            FgtwMessage::FoundNodes { devices } => {
                let mut fields = vec![
                    ("msg_type".to_string(), VsfType::u3(3)),
                    ("device_count".to_string(), VsfType::u(devices.len(), false)),
                ];

                for (i, device) in devices.iter().enumerate() {
                    let prefix = format!("device_{}", i);
                    fields.push((
                        format!("{}_handle_proof", prefix),
                        VsfType::hb(device.handle_proof.to_vec()),
                    ));
                    fields.push((
                        format!("{}_device_pubkey", prefix),
                        device.device_pubkey.to_vsf(),
                    ));
                    fields.push((
                        format!("{}_ip", prefix),
                        VsfType::v_u3(Vector {
                            data: socketaddr_to_bytes(&device.ip),
                        }),
                    ));
                    fields.push((
                        format!("{}_last_seen", prefix),
                        VsfType::f6(device.last_seen),
                    ));
                }

                builder.add_section("fgtw", fields).build()
            }
            FgtwMessage::Announce {
                handle_proof,
                device_pubkey,
                port,
            } => builder
                .add_section(
                    "fgtw",
                    vec![
                        ("msg_type".to_string(), VsfType::u3(4)),
                        (
                            "handle_proof".to_string(),
                            VsfType::hb(handle_proof.to_vec()),
                        ),
                        ("device_pubkey".to_string(), device_pubkey.to_vsf()),
                        ("port".to_string(), VsfType::u(*port as usize, false)),
                    ],
                )
                .build(),
            FgtwMessage::Query {
                handle_proof,
                requester_pubkey,
            } => builder
                .add_section(
                    "fgtw",
                    vec![
                        ("msg_type".to_string(), VsfType::u3(5)),
                        (
                            "handle_proof".to_string(),
                            VsfType::hb(handle_proof.to_vec()),
                        ),
                        ("requester_pubkey".to_string(), requester_pubkey.to_vsf()),
                    ],
                )
                .build(),
            FgtwMessage::QueryResponse { devices } => {
                let mut fields = vec![
                    ("msg_type".to_string(), VsfType::u3(6)),
                    ("device_count".to_string(), VsfType::u(devices.len(), false)),
                ];

                for (i, device) in devices.iter().enumerate() {
                    let prefix = format!("device_{}", i);
                    fields.push((
                        format!("{}_handle_proof", prefix),
                        VsfType::hb(device.handle_proof.to_vec()),
                    ));
                    fields.push((
                        format!("{}_device_pubkey", prefix),
                        device.device_pubkey.to_vsf(),
                    ));
                    fields.push((
                        format!("{}_ip", prefix),
                        VsfType::v_u3(Vector {
                            data: socketaddr_to_bytes(&device.ip),
                        }),
                    ));
                    fields.push((
                        format!("{}_last_seen", prefix),
                        VsfType::f6(device.last_seen),
                    ));
                }

                builder.add_section("fgtw", fields).build()
            }
            FgtwMessage::StatusPing {
                timestamp,
                sender_pubkey,
                provenance_hash,
                signature,
            } => {
                // Simplified header-only format: RÅ< ... ke[pubkey] ge[sig] n1 (ping) >
                // All crypto is in header, section just identifies message type
                // Avatar is NOT included - fetched by handle instead
                builder
                    .creation_time_nanos(*timestamp)
                    .provenance_hash(*provenance_hash)
                    .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                    .add_section("ping", vec![])
                    .build()
            }
            FgtwMessage::StatusPong {
                timestamp,
                responder_pubkey,
                provenance_hash,
                signature,
            } => {
                // Simplified header-only format: RÅ< ... ke[pubkey] ge[sig] n1 (pong) >
                // All crypto is in header, section just identifies message type
                // Avatar is NOT included - fetched by handle instead
                builder
                    .creation_time_nanos(*timestamp)
                    .provenance_hash(*provenance_hash)
                    .signature_ed25519(*responder_pubkey.as_bytes(), *signature)
                    .add_section("pong", vec![])
                    .build()
            }
            FgtwMessage::ClutchOffer {
                timestamp,
                from_handle_proof,
                to_handle_proof,
                ephemeral_x25519,
                sender_pubkey,
                signature,
            } => {
                // CLUTCH offer: parallel exchange - both parties send this
                let provenance =
                    compute_clutch_provenance(from_handle_proof, to_handle_proof, ephemeral_x25519);
                builder
                    .creation_time_nanos(*timestamp)
                    .provenance_hash(provenance)
                    .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                    .add_section(
                        "clutch_offer",
                        vec![
                            ("from".to_string(), VsfType::hb(from_handle_proof.to_vec())),
                            ("to".to_string(), VsfType::hb(to_handle_proof.to_vec())),
                            (
                                "ephemeral".to_string(),
                                VsfType::kx(ephemeral_x25519.to_vec()),
                            ),
                        ],
                    )
                    .build()
            }
            FgtwMessage::ClutchInit {
                timestamp,
                from_handle_proof,
                to_handle_proof,
                ephemeral_x25519,
                sender_pubkey,
                signature,
            } => {
                // CLUTCH init (v1 legacy): use section body for handle proofs and ephemeral key
                let provenance =
                    compute_clutch_provenance(from_handle_proof, to_handle_proof, ephemeral_x25519);
                builder
                    .creation_time_nanos(*timestamp)
                    .provenance_hash(provenance)
                    .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                    .add_section(
                        "clutch_init",
                        vec![
                            ("from".to_string(), VsfType::hb(from_handle_proof.to_vec())),
                            ("to".to_string(), VsfType::hb(to_handle_proof.to_vec())),
                            (
                                "ephemeral".to_string(),
                                VsfType::kx(ephemeral_x25519.to_vec()),
                            ),
                        ],
                    )
                    .build()
            }
            FgtwMessage::ClutchResponse {
                timestamp,
                from_handle_proof,
                to_handle_proof,
                ephemeral_x25519,
                sender_pubkey,
                signature,
            } => {
                let provenance =
                    compute_clutch_provenance(from_handle_proof, to_handle_proof, ephemeral_x25519);
                builder
                    .creation_time_nanos(*timestamp)
                    .provenance_hash(provenance)
                    .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                    .add_section(
                        "clutch_resp",
                        vec![
                            ("from".to_string(), VsfType::hb(from_handle_proof.to_vec())),
                            ("to".to_string(), VsfType::hb(to_handle_proof.to_vec())),
                            (
                                "ephemeral".to_string(),
                                VsfType::kx(ephemeral_x25519.to_vec()),
                            ),
                        ],
                    )
                    .build()
            }
            FgtwMessage::ClutchComplete {
                timestamp,
                from_handle_proof,
                to_handle_proof,
                proof,
                sender_pubkey,
                signature,
            } => {
                let provenance =
                    compute_clutch_complete_provenance(from_handle_proof, to_handle_proof, proof);
                builder
                    .creation_time_nanos(*timestamp)
                    .provenance_hash(provenance)
                    .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                    .add_section(
                        "clutch_done",
                        vec![
                            ("from".to_string(), VsfType::hb(from_handle_proof.to_vec())),
                            ("to".to_string(), VsfType::hb(to_handle_proof.to_vec())),
                            ("proof".to_string(), VsfType::hb(proof.to_vec())),
                        ],
                    )
                    .build()
            }
            FgtwMessage::ChatMessage {
                timestamp,
                from_handle_proof,
                sequence,
                ciphertext,
                sender_pubkey,
                signature,
            } => {
                let provenance = compute_msg_provenance(from_handle_proof, *sequence);
                builder
                    .creation_time_nanos(*timestamp)
                    .provenance_hash(provenance)
                    .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                    .add_section(
                        "msg",
                        vec![
                            ("from".to_string(), VsfType::hb(from_handle_proof.to_vec())),
                            ("seq".to_string(), VsfType::u(*sequence as usize, false)),
                            (
                                "data".to_string(),
                                VsfType::t_u3(vsf::Tensor::new(
                                    vec![ciphertext.len()],
                                    ciphertext.clone(),
                                )),
                            ),
                        ],
                    )
                    .build()
            }
            FgtwMessage::MessageAck {
                timestamp,
                from_handle_proof,
                sequence,
                plaintext_hash,
                sender_last_acked,
                sender_pubkey,
                signature,
            } => {
                let provenance = compute_ack_provenance(
                    from_handle_proof,
                    *sequence,
                    plaintext_hash,
                    sender_last_acked,
                );
                builder
                    .creation_time_nanos(*timestamp)
                    .provenance_hash(provenance)
                    .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                    .add_section(
                        "ack",
                        vec![
                            ("from".to_string(), VsfType::hb(from_handle_proof.to_vec())),
                            ("seq".to_string(), VsfType::u(*sequence as usize, false)),
                            ("hash".to_string(), VsfType::hb(plaintext_hash.to_vec())),
                            ("weave".to_string(), VsfType::hb(sender_last_acked.to_vec())),
                        ],
                    )
                    .build()
            }
        };

        result.unwrap_or_else(|e| {
            crate::log_error(&format!("FGTW: Failed to build VSF message: {}", e));
            Vec::new()
        })
    }

    /// Deserialize from proper VSF file
    pub fn from_vsf_bytes(bytes: &[u8]) -> Result<Self, String> {
        // Check magic number FIRST (reject non-VSF immediately)
        if bytes.len() < 4 {
            return Err("Message too short".to_string());
        }

        if &bytes[0..3] != "RÅ".as_bytes() || bytes[3] != b'<' {
            return Err("Not a VSF file (invalid magic)".to_string());
        }

        // Use library's VsfHeader::decode() for proper VSF v4 parsing
        use vsf::file_format::VsfHeader;
        use vsf::parse;

        let (header, header_end) =
            VsfHeader::decode(bytes).map_err(|e| format!("Failed to parse VSF header: {}", e))?;

        // ptr is now right after '>'
        let mut ptr = header_end;

        // Check for empty section (header-only format like ping/pong)
        // Empty sections have no '[' after '>' - the section name is in the header field
        if ptr >= bytes.len() || bytes[ptr] != b'[' {
            // No section body - search header fields for message type (ping/pong)
            let section_name = header
                .fields
                .iter()
                .find(|f| f.name == "ping" || f.name == "pong")
                .map(|f| f.name.as_str());

            if let Some(section_name) = section_name {
                let timestamp = extract_header_timestamp(&header)?;
                let pubkey = extract_header_pubkey(&header)?;
                let provenance_hash = extract_header_provenance(&header)?;
                let signature = extract_header_signature(&header)?;

                if section_name == "ping" {
                    return Ok(FgtwMessage::StatusPing {
                        timestamp,
                        sender_pubkey: pubkey,
                        provenance_hash,
                        signature,
                    });
                } else {
                    return Ok(FgtwMessage::StatusPong {
                        timestamp,
                        responder_pubkey: pubkey,
                        provenance_hash,
                        signature,
                    });
                }
            }
            return Err("No section found".to_string());
        }

        ptr += 1; // Skip '['

        // Parse section name from section body
        let section_name = match parse(bytes, &mut ptr) {
            Ok(VsfType::d(name)) => name,
            _ => return Err("Invalid section name".to_string()),
        };

        // Handle simplified ping/pong format (section name is ping/pong) - legacy path
        if section_name == "ping" || section_name == "pong" {
            // Extract from header: timestamp, pubkey, provenance_hash, signature
            let timestamp = extract_header_timestamp(&header)?;
            let pubkey = extract_header_pubkey(&header)?;
            let provenance_hash = extract_header_provenance(&header)?;
            let signature = extract_header_signature(&header)?;

            if section_name == "ping" {
                return Ok(FgtwMessage::StatusPing {
                    timestamp,
                    sender_pubkey: pubkey,
                    provenance_hash,
                    signature,
                });
            } else {
                return Ok(FgtwMessage::StatusPong {
                    timestamp,
                    responder_pubkey: pubkey,
                    provenance_hash,
                    signature,
                });
            }
        }

        // Handle CLUTCH messages (clutch_offer, clutch_init, clutch_resp, clutch_done)
        if section_name == "clutch_offer"
            || section_name == "clutch_init"
            || section_name == "clutch_resp"
            || section_name == "clutch_done"
        {
            let timestamp = extract_header_timestamp(&header)?;
            let sender_pubkey = extract_header_pubkey(&header)?;
            let signature = extract_header_signature(&header)?;

            // Parse section fields
            let mut fields: Vec<(String, VsfType)> = Vec::new();
            while ptr < bytes.len() && bytes[ptr] != b']' {
                if bytes[ptr] != b'(' {
                    return Err("Expected field start '('".to_string());
                }
                ptr += 1;
                let field_name = match parse(bytes, &mut ptr) {
                    Ok(VsfType::d(name)) => name,
                    _ => return Err("Invalid CLUTCH field name".to_string()),
                };
                if ptr < bytes.len() && bytes[ptr] == b':' {
                    ptr += 1;
                    let value =
                        parse(bytes, &mut ptr).map_err(|e| format!("Parse CLUTCH field: {}", e))?;
                    fields.push((field_name, value));
                }
                if ptr >= bytes.len() || bytes[ptr] != b')' {
                    return Err("Expected field end ')'".to_string());
                }
                ptr += 1;
            }

            let from_handle_proof = extract_hash(&fields, "from")?;
            let to_handle_proof = extract_hash(&fields, "to")?;

            if section_name == "clutch_offer" {
                // Parallel v2 offer
                let ephemeral = extract_clutch_ephemeral(&fields)?;
                return Ok(FgtwMessage::ClutchOffer {
                    timestamp,
                    from_handle_proof,
                    to_handle_proof,
                    ephemeral_x25519: ephemeral,
                    sender_pubkey,
                    signature,
                });
            } else if section_name == "clutch_init" {
                // Legacy v1 init
                let ephemeral = extract_clutch_ephemeral(&fields)?;
                return Ok(FgtwMessage::ClutchInit {
                    timestamp,
                    from_handle_proof,
                    to_handle_proof,
                    ephemeral_x25519: ephemeral,
                    sender_pubkey,
                    signature,
                });
            } else if section_name == "clutch_resp" {
                // Legacy v1 response
                let ephemeral = extract_clutch_ephemeral(&fields)?;
                return Ok(FgtwMessage::ClutchResponse {
                    timestamp,
                    from_handle_proof,
                    to_handle_proof,
                    ephemeral_x25519: ephemeral,
                    sender_pubkey,
                    signature,
                });
            } else {
                // clutch_done
                let proof = extract_hash(&fields, "proof")?;
                return Ok(FgtwMessage::ClutchComplete {
                    timestamp,
                    from_handle_proof,
                    to_handle_proof,
                    proof,
                    sender_pubkey,
                    signature,
                });
            }
        }

        // Handle msg (encrypted chat message) and ack (acknowledgment)
        if section_name == "msg" || section_name == "ack" {
            let timestamp = extract_header_timestamp(&header)?;
            let sender_pubkey = extract_header_pubkey(&header)?;
            let signature = extract_header_signature(&header)?;

            // Parse section fields
            let mut fields: Vec<(String, VsfType)> = Vec::new();
            while ptr < bytes.len() && bytes[ptr] != b']' {
                if bytes[ptr] != b'(' {
                    return Err("Expected field start '('".to_string());
                }
                ptr += 1;
                let field_name = match parse(bytes, &mut ptr) {
                    Ok(VsfType::d(name)) => name,
                    _ => return Err("Invalid msg/ack field name".to_string()),
                };
                if ptr < bytes.len() && bytes[ptr] == b':' {
                    ptr += 1;
                    let value = parse(bytes, &mut ptr)
                        .map_err(|e| format!("Parse msg/ack field: {}", e))?;
                    fields.push((field_name, value));
                }
                if ptr >= bytes.len() || bytes[ptr] != b')' {
                    return Err("Expected field end ')'".to_string());
                }
                ptr += 1;
            }

            let from_handle_proof = extract_hash(&fields, "from")?;
            let sequence = extract_sequence(&fields, "seq")?;

            if section_name == "msg" {
                let ciphertext = extract_data(&fields, "data")?;
                return Ok(FgtwMessage::ChatMessage {
                    timestamp,
                    from_handle_proof,
                    sequence,
                    ciphertext,
                    sender_pubkey,
                    signature,
                });
            } else {
                // ack - extract plaintext_hash and weave hash for chain weaving
                let plaintext_hash = extract_hash(&fields, "hash")?;
                let sender_last_acked = extract_hash(&fields, "weave")?;
                return Ok(FgtwMessage::MessageAck {
                    timestamp,
                    from_handle_proof,
                    sequence,
                    plaintext_hash,
                    sender_last_acked,
                    sender_pubkey,
                    signature,
                });
            }
        }

        // Original fgtw section handling
        if section_name != "fgtw" {
            return Err(format!(
                "Expected 'fgtw', 'ping'/'pong', 'clutch_*', 'msg', or 'ack' section, got '{}'",
                section_name
            ));
        }

        // Parse fields into a vec (small N, linear scan faster than hash)
        let mut fields: Vec<(String, VsfType)> = Vec::new();

        while ptr < bytes.len() && bytes[ptr] != b']' {
            if bytes[ptr] != b'(' {
                return Err("Expected field start '('".to_string());
            }
            ptr += 1;

            // Parse field name
            let field_name = match parse(bytes, &mut ptr) {
                Ok(VsfType::d(name)) => name,
                _ => return Err("Invalid field name".to_string()),
            };

            // Check for value (colon means there's a value)
            if ptr < bytes.len() && bytes[ptr] == b':' {
                ptr += 1;
                let value =
                    parse(bytes, &mut ptr).map_err(|e| format!("Parse field value: {}", e))?;
                fields.push((field_name, value));
            }

            // Skip closing ')'
            if ptr >= bytes.len() || bytes[ptr] != b')' {
                return Err("Expected field end ')'".to_string());
            }
            ptr += 1;
        }

        // Extract msg_type
        let msg_type = match get_field(&fields, "msg_type") {
            Some(vsf_val) => {
                u8::from_vsf_type(vsf_val).map_err(|e| format!("Invalid msg_type: {}", e))?
            }
            None => return Err("Missing msg_type".to_string()),
        };

        // Reconstruct message based on type
        match msg_type {
            0 => {
                // Ping
                let device_pubkey = extract_pubkey(&fields, "device_pubkey")?;
                Ok(FgtwMessage::Ping { device_pubkey })
            }
            1 => {
                // Pong
                let device_pubkey = extract_pubkey(&fields, "device_pubkey")?;
                let peers = extract_peer_list(&fields, "peer")?;
                Ok(FgtwMessage::Pong {
                    device_pubkey,
                    peers,
                })
            }
            2 => {
                // FindNode
                let handle_proof = extract_hash(&fields, "handle_proof")?;
                let requester_pubkey = extract_pubkey(&fields, "requester_pubkey")?;
                Ok(FgtwMessage::FindNode {
                    handle_proof,
                    requester_pubkey,
                })
            }
            3 => {
                // FoundNodes
                let devices = extract_peer_list(&fields, "device")?;
                Ok(FgtwMessage::FoundNodes { devices })
            }
            4 => {
                // Announce
                let handle_proof = extract_hash(&fields, "handle_proof")?;
                let device_pubkey = extract_pubkey(&fields, "device_pubkey")?;
                let port = match get_field(&fields, "port") {
                    Some(vsf_val) => {
                        u16::from_vsf_type(vsf_val).map_err(|e| format!("Invalid port: {}", e))?
                    }
                    None => return Err("Missing port".to_string()),
                };
                Ok(FgtwMessage::Announce {
                    handle_proof,
                    device_pubkey,
                    port,
                })
            }
            5 => {
                // Query
                let handle_proof = extract_hash(&fields, "handle_proof")?;
                let requester_pubkey = extract_pubkey(&fields, "requester_pubkey")?;
                Ok(FgtwMessage::Query {
                    handle_proof,
                    requester_pubkey,
                })
            }
            6 => {
                // QueryResponse
                let devices = extract_peer_list(&fields, "device")?;
                Ok(FgtwMessage::QueryResponse { devices })
            }
            _ => Err(format!("Unknown message type: {}", msg_type)),
        }
    }
}

impl PeerRecord {
    pub fn new(handle_proof: [u8; 32], device_pubkey: DevicePubkey, ip: SocketAddr) -> Self {
        Self {
            handle_proof,
            device_pubkey,
            ip,
            last_seen: vsf::eagle_time_nanos(),
        }
    }
}

// Helper to get field from Vec (linear scan, faster than HashMap for small N)
fn get_field<'a>(fields: &'a [(String, VsfType)], key: &str) -> Option<&'a VsfType> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

// Helper functions for extracting fields from VSF
fn extract_hash(fields: &[(String, VsfType)], key: &str) -> Result<[u8; 32], String> {
    let hash_bytes = match get_field(fields, key) {
        Some(VsfType::hb(bytes)) => bytes,
        _ => return Err(format!("Missing or invalid hash: {}", key)),
    };
    let mut arr = [0u8; 32];
    if hash_bytes.len() != 32 {
        return Err(format!("Hash {} must be 32 bytes", key));
    }
    arr.copy_from_slice(hash_bytes);
    Ok(arr)
}

fn extract_pubkey(fields: &[(String, VsfType)], key: &str) -> Result<DevicePubkey, String> {
    // DevicePubkey is Ed25519 (ke), not X25519 (kx)
    let pubkey_bytes = match get_field(fields, key) {
        Some(VsfType::ke(bytes)) => bytes,
        _ => return Err(format!("Missing or invalid pubkey: {}", key)),
    };
    let mut pubkey_arr = [0u8; 32];
    if pubkey_bytes.len() != 32 {
        return Err(format!("Pubkey {} must be 32 bytes", key));
    }
    pubkey_arr.copy_from_slice(pubkey_bytes);
    Ok(DevicePubkey::from_bytes(pubkey_arr))
}

fn extract_clutch_ephemeral(fields: &[(String, VsfType)]) -> Result<[u8; 32], String> {
    let ephemeral_bytes = match get_field(fields, "ephemeral") {
        Some(VsfType::kx(bytes)) => bytes,
        _ => return Err("Missing or invalid ephemeral key".to_string()),
    };
    let mut arr = [0u8; 32];
    if ephemeral_bytes.len() != 32 {
        return Err("Ephemeral key must be 32 bytes".to_string());
    }
    arr.copy_from_slice(ephemeral_bytes);
    Ok(arr)
}

fn extract_sequence(fields: &[(String, VsfType)], key: &str) -> Result<u64, String> {
    match get_field(fields, key) {
        Some(VsfType::u(v, _)) => Ok(*v as u64),
        Some(VsfType::u3(v)) => Ok(*v as u64),
        Some(VsfType::u4(v)) => Ok(*v as u64),
        Some(VsfType::u5(v)) => Ok(*v as u64),
        Some(VsfType::u6(v)) => Ok(*v),
        _ => Err(format!("Missing or invalid sequence: {}", key)),
    }
}

fn extract_data(fields: &[(String, VsfType)], key: &str) -> Result<Vec<u8>, String> {
    match get_field(fields, key) {
        Some(VsfType::t_u3(tensor)) => Ok(tensor.data.clone()),
        _ => Err(format!("Missing or invalid data: {}", key)),
    }
}

fn extract_peer_list(
    fields: &[(String, VsfType)],
    prefix: &str,
) -> Result<Vec<PeerRecord>, String> {
    let count_key = format!("{}_count", prefix);
    let count = match get_field(fields, &count_key) {
        Some(vsf_val) => {
            usize::from_vsf_type(vsf_val).map_err(|e| format!("Invalid {}_count: {}", prefix, e))?
        }
        None => return Err(format!("Missing {}_count", prefix)),
    };

    let mut peers = Vec::with_capacity(count);
    for i in 0..count {
        let peer_prefix = format!("{}_{}", prefix, i);

        let handle_proof = extract_hash(fields, &format!("{}_handle_proof", peer_prefix))?;
        let device_pubkey = extract_pubkey(fields, &format!("{}_device_pubkey", peer_prefix))?;

        let ip_key = format!("{}_ip", peer_prefix);
        let ip_bytes = match get_field(fields, &ip_key) {
            Some(VsfType::v_u3(vec)) => &vec.data,
            _ => return Err(format!("Missing or invalid {}", ip_key)),
        };
        let ip = bytes_to_socketaddr(ip_bytes)
            .ok_or_else(|| format!("Invalid IP bytes for {}", ip_key))?;

        let last_seen_key = format!("{}_last_seen", peer_prefix);
        let last_seen = match get_field(fields, &last_seen_key) {
            Some(VsfType::f6(v)) => *v,
            _ => return Err(format!("Missing or invalid {}", last_seen_key)),
        };

        peers.push(PeerRecord {
            handle_proof,
            device_pubkey,
            ip,
            last_seen,
        });
    }

    Ok(peers)
}

// Helper functions to extract from VsfHeader for simplified ping/pong format

fn extract_header_timestamp(header: &vsf::file_format::VsfHeader) -> Result<f64, String> {
    use vsf::types::EtType;
    match &header.creation_time {
        VsfType::e(EtType::f6(v)) => Ok(*v),
        VsfType::e(EtType::f5(v)) => Ok(*v as f64),
        VsfType::e(EtType::u(v)) => Ok(*v as f64),
        VsfType::e(EtType::i(v)) => Ok(*v as f64),
        _ => Err("Invalid header timestamp".to_string()),
    }
}

fn extract_header_provenance(header: &vsf::file_format::VsfHeader) -> Result<[u8; 32], String> {
    match &header.provenance_hash {
        VsfType::hp(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(bytes);
            Ok(arr)
        }
        _ => Err("Invalid or missing header provenance hash".to_string()),
    }
}

fn extract_header_pubkey(header: &vsf::file_format::VsfHeader) -> Result<DevicePubkey, String> {
    if let Some(ref pubkey) = header.signer_pubkey {
        match pubkey {
            VsfType::ke(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                return Ok(DevicePubkey::from_bytes(arr));
            }
            _ => {}
        }
    }
    Err("Invalid or missing header signer pubkey".to_string())
}

fn extract_header_signature(header: &vsf::file_format::VsfHeader) -> Result<[u8; 64], String> {
    // Signature is in header.signature field (replaces rolling_hash when present)
    // For now, check if there's a ge signature in the header
    // The VsfHeader struct stores signature in a specific field
    if let Some(ref sig) = header.signature {
        match sig {
            VsfType::ge(bytes) if bytes.len() == 64 => {
                let mut arr = [0u8; 64];
                arr.copy_from_slice(bytes);
                return Ok(arr);
            }
            _ => {}
        }
    }
    Err("Invalid or missing header signature".to_string())
}

// Note: extract_header_avatar_id removed - avatar is now fetched by handle
// Storage key = BLAKE3(BLAKE3(handle) || "avatar")

/// Compute provenance hash for CLUTCH init/response messages
/// provenance = BLAKE3(from_handle_proof || to_handle_proof || ephemeral_pubkey)
fn compute_clutch_provenance(
    from_handle_proof: &[u8; 32],
    to_handle_proof: &[u8; 32],
    ephemeral_pubkey: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(from_handle_proof);
    hasher.update(to_handle_proof);
    hasher.update(ephemeral_pubkey);
    *hasher.finalize().as_bytes()
}

/// Compute provenance hash for CLUTCH complete message
/// provenance = BLAKE3(from_handle_proof || to_handle_proof || proof)
fn compute_clutch_complete_provenance(
    from_handle_proof: &[u8; 32],
    to_handle_proof: &[u8; 32],
    proof: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(from_handle_proof);
    hasher.update(to_handle_proof);
    hasher.update(proof);
    *hasher.finalize().as_bytes()
}

/// Compute provenance hash for encrypted message
/// provenance = BLAKE3(from_handle_proof || sequence)
fn compute_msg_provenance(from_handle_proof: &[u8; 32], sequence: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(from_handle_proof);
    hasher.update(&sequence.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// Compute provenance hash for message acknowledgment
/// provenance = BLAKE3(from_handle_proof || sequence || plaintext_hash || weave_hash || "ack")
fn compute_ack_provenance(
    from_handle_proof: &[u8; 32],
    sequence: u64,
    plaintext_hash: &[u8; 32],
    weave_hash: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(from_handle_proof);
    hasher.update(&sequence.to_le_bytes());
    hasher.update(plaintext_hash);
    hasher.update(weave_hash);
    hasher.update(b"ack");
    *hasher.finalize().as_bytes()
}
