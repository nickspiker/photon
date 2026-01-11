use crate::types::DevicePubkey;
use std::net::{IpAddr, SocketAddr};
use vsf::schema::{FromVsfType, SectionBuilder, SectionSchema, TypeConstraint};
use vsf::types::Vector;
use vsf::VsfType;

// ============================================================================
// VSF Schemas for FGTW Protocol Messages
// ============================================================================

/// Schema for pong section with sync records
fn pong_schema() -> SectionSchema {
    SectionSchema::new("pong")
        .field("conversation_token", TypeConstraint::Any)
        .field("last_received_ef6", TypeConstraint::AnyFloat)
        .field("sync_count", TypeConstraint::AnyUnsigned) // Number of sync records
}

/// Schema for msg (chat message) section
fn msg_schema() -> SectionSchema {
    SectionSchema::new("msg")
        .field("tok", TypeConstraint::Any) // conversation_token (hg)
        .field("prev", TypeConstraint::Any) // prev_msg_hp (hp)
        .field("data", TypeConstraint::Any) // ciphertext (v)
        .field("time", TypeConstraint::AnyFloat) // eagle_time (f6)
        .field("hash", TypeConstraint::Any) // plaintext_hash (hb)
}

/// Schema for ack (message acknowledgment) section
fn ack_schema() -> SectionSchema {
    SectionSchema::new("ack")
        .field("tok", TypeConstraint::Any) // conversation_token (hg)
        .field("time", TypeConstraint::AnyFloat) // acked_eagle_time (f6)
        .field("hash", TypeConstraint::Any) // plaintext_hash (hb)
}

/// Schema for clutch_offer section
fn clutch_offer_schema() -> SectionSchema {
    SectionSchema::new("clutch_offer")
        .field("tok", TypeConstraint::Any) // conversation_token (hg)
        .field("pubkeys", TypeConstraint::Any) // Multi-value pubkeys (kx, kp, kk, kf, kn, kl, kh)
}

/// Schema for clutch_kem_response section
fn clutch_kem_response_schema() -> SectionSchema {
    SectionSchema::new("clutch_kem_response")
        .field("tok", TypeConstraint::Any) // conversation_token (hg)
        .field("target_hqc", TypeConstraint::Any) // HQC public prefix (hb)
        .field("ciphertexts", TypeConstraint::Any) // Multi-value ciphertexts (vf, vn, vl, vc)
        .field("ephemerals", TypeConstraint::Any) // Multi-value ephemerals (kx, kp, kk, kp)
}

/// Schema for clutch_complete section
fn clutch_complete_schema() -> SectionSchema {
    SectionSchema::new("clutch_complete")
        .field("tok", TypeConstraint::Any) // conversation_token (hg)
        .field("eggs_proof", TypeConstraint::Any) // EGGS proof (hg)
}

/// Schema for fgtw section (multiple message types)
fn fgtw_schema() -> SectionSchema {
    SectionSchema::new("fgtw")
        .field("msg_type", TypeConstraint::AnyUnsigned)
        .field("device_pubkey", TypeConstraint::Ed25519Key)
        .field("handle_proof", TypeConstraint::Any) // Hash
        .field("requester_pubkey", TypeConstraint::Ed25519Key)
        .field("peer", TypeConstraint::Any) // Multi-value peer list
        .field("device", TypeConstraint::Any) // Multi-value device list
        .field("port", TypeConstraint::AnyUnsigned)
}

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
    /// Format:
    /// RÅ< z4 y2 ef6[timestamp] hp[SAME provenance] ke[pubkey] ge[signature] n1 (pong) >
    /// [pong (sync_count: N) (sync_0_tok: hb) (sync_0_ef6: f6) ...]
    ///
    /// - Echoes same provenance_hash from ping (proves we saw it)
    /// - ke = responder's Ed25519 public key (for signature verification)
    /// - ge = signature of provenance_hash (proves we processed it)
    /// - sync records: Per-conversation last_received_ef6 for efficient resync
    ///   Peer can retransmit everything after that timestamp
    ///
    /// Note: Avatar is fetched by handle, not exchanged in ping/pong.
    /// Storage key = BLAKE3(BLAKE3(handle) || "avatar")
    StatusPong {
        timestamp: f64,                 // Responder's current Eagle time (ef6)
        responder_pubkey: DevicePubkey, // Who is responding
        provenance_hash: [u8; 32],      // Same hash from ping (proves we received it)
        signature: [u8; 64],            // Ed25519 signature of provenance_hash
        /// Per-conversation sync records: (conversation_token, last_received_ef6)
        /// Tells peer: "For this conversation, your last message I received was at time X"
        /// Peer retransmits any pending messages with eagle_time > X
        sync_records: Vec<SyncRecord>,
    },
    // NOTE: ClutchOffer, ClutchInit, ClutchResponse, ClutchComplete REMOVED
    // Full 8-primitive CLUTCH uses ClutchOffer and ClutchKemResponse
    // which are handled via build_clutch_offer_vsf() and parse_clutch_offer_vsf()
    // See CLUTCH.md Section 4.2 for the slot-based ceremony protocol.
    /// Encrypted chat message
    ///
    /// Format: section "msg" with encrypted payload per CHAIN.md Section 6.2
    /// - conversation_token: smear_hash(sorted participant identity seeds) - privacy-preserving
    /// - prev_msg_hp: hash chain link to previous message (or first_message_anchor)
    /// - ciphertext: encrypted [x(text), hM(confirm_smear)] section
    ChatMessage {
        timestamp: f64,
        /// Privacy-preserving conversation token (smear_hash of sorted participant seeds)
        conversation_token: [u8; 32],
        prev_msg_hp: [u8; 32],
        ciphertext: Vec<u8>,
        sender_pubkey: DevicePubkey,
        signature: [u8; 64],
    },
    /// Message acknowledgment
    ///
    /// Confirms receipt of a message by eagle_time (no sequence numbers).
    /// Per CHAIN.md Section 6.1:
    /// - acked_eagle_time: which message we're ACKing (f64 from their header)
    /// - plaintext_hash: proves we decrypted correctly (BLAKE3 of decrypted content)
    MessageAck {
        timestamp: f64,
        /// Privacy-preserving conversation token (smear_hash of sorted participant seeds)
        conversation_token: [u8; 32],
        /// Eagle time of the message being ACKed (from their VSF header)
        acked_eagle_time: f64,
        /// BLAKE3 hash of decrypted plaintext - proves we decrypted correctly
        plaintext_hash: [u8; 32],
        sender_pubkey: DevicePubkey,
        signature: [u8; 64],
    },
}

/// Peer record - one device for a user handle
#[derive(Debug, Clone)]
pub struct PeerRecord {
    pub handle_proof: [u8; 32], // Memory-hard PoW output (24MB, 17 rounds)
    pub device_pubkey: DevicePubkey, // Device's X25519 public key (used as device identifier)
    pub ip: SocketAddr,         // Where to reach this device (public IP)
    pub local_ip: Option<std::net::IpAddr>, // LAN IP for hairpin NAT (peers behind same public IP)
    pub last_seen: f64,         // Timestamp (f64, serializes as VSF type f6)
}

/// Sync record for pong - tells peer our last received message timestamp per conversation
/// Used for efficient resync: peer retransmits pending messages with eagle_time > last_received_ef6
#[derive(Debug, Clone)]
pub struct SyncRecord {
    /// Privacy-preserving conversation token (smear_hash of sorted participant seeds)
    pub conversation_token: [u8; 32],
    /// Eagle time of last message received from peer in this conversation
    /// Peer should retransmit any pending messages with eagle_time > this value
    pub last_received_ef6: f64,
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

        let builder = VsfBuilder::new().creation_time_nanos(vsf::eagle_time_nanos());

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
                        VsfType::hP(peer.handle_proof.to_vec()),
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
                            VsfType::hP(handle_proof.to_vec()),
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
                        VsfType::hP(device.handle_proof.to_vec()),
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
                            VsfType::hP(handle_proof.to_vec()),
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
                            VsfType::hP(handle_proof.to_vec()),
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
                        VsfType::hP(device.handle_proof.to_vec()),
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
                sync_records,
            } => {
                // Pong with sync records for efficient resync
                // Format: RÅ< ... ke[pubkey] ge[sig] > [pong (sync_count: N) (sync_0_tok: hb) (sync_0_ef6: f6) ...]
                let mut fields = vec![(
                    "sync_count".to_string(),
                    VsfType::u(sync_records.len(), false),
                )];
                for (i, record) in sync_records.iter().enumerate() {
                    fields.push((
                        format!("sync_{}_tok", i),
                        VsfType::hb(record.conversation_token.to_vec()),
                    ));
                    fields.push((
                        format!("sync_{}_ef6", i),
                        VsfType::f6(record.last_received_ef6),
                    ));
                }
                builder
                    .creation_time_nanos(*timestamp)
                    .provenance_hash(*provenance_hash)
                    .signature_ed25519(*responder_pubkey.as_bytes(), *signature)
                    .add_section("pong", fields)
                    .build()
            }
            // NOTE: ClutchOffer, ClutchInit, ClutchResponse, ClutchComplete serialization REMOVED
            // Full CLUTCH uses build_clutch_offer_vsf() and build_clutch_kem_response_vsf()
            FgtwMessage::ChatMessage {
                timestamp,
                conversation_token,
                prev_msg_hp,
                ciphertext,
                sender_pubkey,
                signature,
            } => {
                // Provenance: BLAKE3(conversation_token || prev_msg_hp)
                let provenance = compute_chat_provenance(conversation_token, prev_msg_hp);
                builder
                    .creation_time_nanos(*timestamp)
                    .provenance_hash(provenance)
                    .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                    .add_section(
                        "msg",
                        vec![
                            ("tok".to_string(), VsfType::hg(conversation_token.to_vec())),
                            ("prev".to_string(), VsfType::hp(prev_msg_hp.to_vec())),
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
                conversation_token,
                acked_eagle_time,
                plaintext_hash,
                sender_pubkey,
                signature,
            } => {
                // Provenance: BLAKE3(conversation_token || acked_eagle_time || plaintext_hash)
                let provenance = compute_ack_provenance_v2(
                    conversation_token,
                    *acked_eagle_time,
                    plaintext_hash,
                );
                builder
                    .creation_time_nanos(*timestamp)
                    .provenance_hash(provenance)
                    .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                    .add_section(
                        "ack",
                        vec![
                            ("tok".to_string(), VsfType::hg(conversation_token.to_vec())),
                            (
                                "time".to_string(),
                                VsfType::e(vsf::types::EtType::f6(*acked_eagle_time)),
                            ),
                            ("hash".to_string(), VsfType::hb(plaintext_hash.to_vec())),
                        ],
                    )
                    .build()
            }
        };

        result.unwrap_or_else(|e| {
            crate::log(&format!("FGTW: Failed to build VSF message: {}", e));
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
                    // Old header-only pong format - no sync records (backwards compat)
                    return Ok(FgtwMessage::StatusPong {
                        timestamp,
                        responder_pubkey: pubkey,
                        provenance_hash,
                        signature,
                        sync_records: vec![],
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

        // Handle ping/pong format
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
                // Pong - parse sync records from section body
                let section_bytes = &bytes[header_end..];
                let schema = pong_schema();
                let builder = SectionBuilder::parse(schema, section_bytes)
                    .map_err(|e| format!("Parse pong: {}", e))?;

                // Convert to fields vec for extract_sync_records
                let mut fields: Vec<(String, VsfType)> = Vec::new();
                for field_name in ["conversation_token", "last_received_ef6"] {
                    if let Ok(values) = builder.get(field_name) {
                        for value in values {
                            fields.push((field_name.to_string(), value.clone()));
                        }
                    }
                }

                // Extract sync records
                let sync_records = extract_sync_records(&fields)?;

                return Ok(FgtwMessage::StatusPong {
                    timestamp,
                    responder_pubkey: pubkey,
                    provenance_hash,
                    signature,
                    sync_records,
                });
            }
        }

        // NOTE: clutch_offer, clutch_init, clutch_resp, clutch_done deserialization REMOVED
        // Full CLUTCH uses parse_clutch_offer_vsf() and parse_clutch_kem_response_vsf()
        // which handle "clutch_offer" and "clutch_kem_response" sections

        // Handle msg (encrypted chat message) and ack (acknowledgment)
        if section_name == "msg" || section_name == "ack" {
            let timestamp = extract_header_timestamp(&header)?;
            let sender_pubkey = extract_header_pubkey(&header)?;
            let signature = extract_header_signature(&header)?;

            // Parse section with schema
            let section_bytes = &bytes[header_end..];
            let schema = if section_name == "msg" { msg_schema() } else { ack_schema() };
            let builder = SectionBuilder::parse(schema, section_bytes)
                .map_err(|e| format!("Parse {}: {}", section_name, e))?;

            // Convert to fields vec for existing extract_* functions
            let mut fields: Vec<(String, VsfType)> = Vec::new();
            for field_name in ["tok", "prev", "data", "time", "hash"] {
                if let Ok(values) = builder.get(field_name) {
                    for value in values {
                        fields.push((field_name.to_string(), value.clone()));
                    }
                }
            }

            // CHAIN format: conversation_token (privacy-preserving), no sequence numbers
            let conversation_token = extract_spaghetti_hash(&fields, "tok")?;

            if section_name == "msg" {
                // ChatMessage: tok (conversation_token), prev (prev_msg_hp), data (ciphertext)
                let prev_msg_hp = extract_hash_hp(&fields, "prev")?;
                let ciphertext = extract_data(&fields, "data")?;
                return Ok(FgtwMessage::ChatMessage {
                    timestamp,
                    conversation_token,
                    prev_msg_hp,
                    ciphertext,
                    sender_pubkey,
                    signature,
                });
            } else {
                // MessageAck: tok (conversation_token), time (acked_eagle_time), hash (plaintext_hash)
                // No sequence numbers, no weave (deferred)
                let acked_eagle_time = extract_eagle_time(&fields, "time")?;
                let plaintext_hash = extract_hash(&fields, "hash")?;
                return Ok(FgtwMessage::MessageAck {
                    timestamp,
                    conversation_token,
                    acked_eagle_time,
                    plaintext_hash,
                    sender_pubkey,
                    signature,
                });
            }
        }

        // FGTW section handling
        if section_name != "fgtw" {
            return Err(format!(
                "Expected 'fgtw', 'ping'/'pong', 'clutch_*', 'msg', or 'ack' section, got '{}'",
                section_name
            ));
        }

        // Parse section with schema validation
        let section_bytes = &bytes[header_end..];
        let schema = fgtw_schema();
        let builder = SectionBuilder::parse(schema, section_bytes)
            .map_err(|e| format!("Parse fgtw: {}", e))?;

        // Convert to fields vec for existing extract_* functions
        let mut fields: Vec<(String, VsfType)> = Vec::new();
        for field_name in ["msg_type", "device_pubkey", "handle_proof", "requester_pubkey", "peer", "device", "port"] {
            if let Ok(values) = builder.get(field_name) {
                for value in values {
                    fields.push((field_name.to_string(), value.clone()));
                }
            }
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
            local_ip: None,
            last_seen: vsf::eagle_time_nanos(),
        }
    }
}

fn get_field<'a>(fields: &'a [(String, VsfType)], key: &str) -> Option<&'a VsfType> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

// Helper functions for extracting fields from VSF
fn extract_hash(fields: &[(String, VsfType)], key: &str) -> Result<[u8; 32], String> {
    let hash_bytes = match get_field(fields, key) {
        Some(VsfType::hP(bytes)) => bytes,
        Some(VsfType::hb(bytes)) => bytes, // Blake3 hash (used in ACKs)
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

// NOTE: extract_clutch_ephemeral removed - was only used by legacy ClutchOffer/Init/Response

/// Extract a provenance hash (hp type) as [u8; 32]
fn extract_hash_hp(fields: &[(String, VsfType)], key: &str) -> Result<[u8; 32], String> {
    let hash_bytes = match get_field(fields, key) {
        Some(VsfType::hp(bytes)) => bytes,
        Some(VsfType::hb(bytes)) => bytes, // Also accept hb for backwards compat
        _ => return Err(format!("Missing or invalid provenance hash: {}", key)),
    };
    let mut arr = [0u8; 32];
    if hash_bytes.len() != 32 {
        return Err(format!("Provenance hash {} must be 32 bytes", key));
    }
    arr.copy_from_slice(hash_bytes);
    Ok(arr)
}

/// Extract a spaghetti hash (hg type) as [u8; 32]
fn extract_spaghetti_hash(fields: &[(String, VsfType)], key: &str) -> Result<[u8; 32], String> {
    let hash_bytes = match get_field(fields, key) {
        Some(VsfType::hg(bytes)) => bytes,
        _ => return Err(format!("Missing or invalid spaghetti hash: {}", key)),
    };
    let mut arr = [0u8; 32];
    if hash_bytes.len() != 32 {
        return Err(format!("Spaghetti hash {} must be 32 bytes", key));
    }
    arr.copy_from_slice(hash_bytes);
    Ok(arr)
}

/// Extract Eagle time (f64) from VSF e() type
fn extract_eagle_time(fields: &[(String, VsfType)], key: &str) -> Result<f64, String> {
    use vsf::types::EtType;
    match get_field(fields, key) {
        Some(VsfType::e(EtType::f6(v))) => Ok(*v),
        Some(VsfType::e(EtType::f5(v))) => Ok(*v as f64),
        Some(VsfType::e(EtType::u(v))) => Ok(*v as f64),
        Some(VsfType::e(EtType::i(v))) => Ok(*v as f64),
        Some(VsfType::f6(v)) => Ok(*v), // Also accept raw f6
        _ => Err(format!("Missing or invalid eagle time: {}", key)),
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
            local_ip: None, // Legacy path - local_ip parsed in bootstrap.rs
            last_seen,
        });
    }

    Ok(peers)
}

/// Extract sync records from pong message fields
/// Format: sync_count, sync_0_tok, sync_0_ef6, sync_1_tok, sync_1_ef6, ...
fn extract_sync_records(fields: &[(String, VsfType)]) -> Result<Vec<SyncRecord>, String> {
    // Get count (optional for backwards compat - default to 0)
    let count = match get_field(fields, "sync_count") {
        Some(vsf_val) => {
            usize::from_vsf_type(vsf_val).map_err(|e| format!("Invalid sync_count: {}", e))?
        }
        None => return Ok(vec![]), // No sync records (old format)
    };

    let mut records = Vec::with_capacity(count);
    for i in 0..count {
        let tok_key = format!("sync_{}_tok", i);
        let ef6_key = format!("sync_{}_ef6", i);

        let conversation_token = extract_hash(fields, &tok_key)?;
        let last_received_ef6 = match get_field(fields, &ef6_key) {
            Some(VsfType::f6(v)) => *v,
            _ => return Err(format!("Missing or invalid {}", ef6_key)),
        };

        records.push(SyncRecord {
            conversation_token,
            last_received_ef6,
        });
    }

    Ok(records)
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

// NOTE: compute_clutch_provenance and compute_clutch_complete_provenance REMOVED
// They were only used by the legacy ClutchOffer/ClutchInit/ClutchResponse/ClutchComplete
// Full CLUTCH uses ceremony_id as provenance (deterministic from handle_hashes)

/// Compute provenance hash for encrypted chat message (CHAIN format)
/// provenance = BLAKE3(conversation_token || prev_msg_hp)
fn compute_chat_provenance(conversation_token: &[u8; 32], prev_msg_hp: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(conversation_token);
    hasher.update(prev_msg_hp);
    *hasher.finalize().as_bytes()
}

/// Compute provenance hash for message acknowledgment (CHAIN format)
/// provenance = BLAKE3(conversation_token || acked_eagle_time_bytes || plaintext_hash || "ack")
fn compute_ack_provenance_v2(
    conversation_token: &[u8; 32],
    acked_eagle_time: f64,
    plaintext_hash: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(conversation_token);
    hasher.update(&acked_eagle_time.to_le_bytes());
    hasher.update(plaintext_hash);
    hasher.update(b"ack");
    *hasher.finalize().as_bytes()
}

// =============================================================================
// VSF-WRAPPED CLUTCH MESSAGES (Full 8-Algorithm CLUTCH)
// =============================================================================

use crate::crypto::clutch::{ClutchCompletePayload, ClutchKemResponsePayload, ClutchOfferPayload};

// NOTE: ceremony_id is now computed deterministically via CeremonyId::derive()
// from sorted participant handle_hashes. No memory-hard hashing needed.
// See src/types/friendship.rs for the implementation.

/// Build a signed VSF ClutchOffer message (~548KB).
///
/// Uses VSF native key types for all 8 algorithms:
/// - kx: X25519 (32B)
/// - kp: P-384 (97B) and P-256 (65B) - size disambiguates
/// - kk: secp256k1 (33B compressed or 65B uncompressed)
/// - kf: FrodoKEM-976 (15632B)
/// - kn: NTRU-HRSS-701 (1138B)
/// - kl: Classic McEliece-460896 (~512KB)
/// - kh: HQC-256 (7285B)
///
/// The ceremony_id is deterministic from CeremonyId::derive(&[handle_hashes]).
/// Both parties compute the same value independently - no echo needed.
///
/// Returns signed VSF bytes ready for transmission.
///
/// conversation_token: Privacy-preserving smear_hash of sorted participant identity seeds.
/// Replaces handle_hashes to prevent identity correlation by network observers.
///
/// Note: The offer_provenance is computed deterministically from the public keys.
/// This ensures both parties compute identical provenances regardless of timestamp.
/// The ceremony_id is computed later from all parties' offer provenances via spaghettify.
/// Returns (vsf_bytes, offer_provenance) - the signed VSF and the key-based provenance.
pub fn build_clutch_offer_vsf(
    conversation_token: &[u8; 32],
    payload: &ClutchOfferPayload,
    device_pubkey: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<(Vec<u8>, [u8; 32]), String> {
    use vsf::VsfBuilder;

    // Build unsigned VSF with signature placeholder
    // hp (provenance hash) will be auto-computed by sign_file from the content
    // This hash is unique per offer due to timestamp and content
    use vsf::file_format::VsfSection;

    // Build section with multi-value field (matches keypairs.vsf format)
    let mut section = VsfSection::new("clutch_offer");
    section.add_field("tok", VsfType::hg(conversation_token.to_vec()));
    // All 8 pubkeys as multi-value field (kx, kp, kk, kp, kf, kn, kl, kh order)
    section.add_field_multi(
        "pubkeys",
        vec![
            VsfType::kx(payload.x25519_public.to_vec()),
            VsfType::kp(payload.p384_public.clone()),
            VsfType::kk(payload.secp256k1_public.clone()),
            VsfType::kp(payload.p256_public.clone()),
            VsfType::kf(payload.frodo976_public.clone()),
            VsfType::kn(payload.ntru701_public.clone()),
            VsfType::kl(payload.mceliece_public.clone()),
            VsfType::kh(payload.hqc256_public.clone()),
        ],
    );

    let unsigned = VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .signature_ed25519(*device_pubkey, [0u8; 64])
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("Failed to build ClutchOffer VSF: {}", e))?;

    // Sign the file (computes file hash, signs it, patches ge)
    let signed = vsf::verification::sign_file(unsigned, device_secret)?;

    // Compute offer_provenance from keys (deterministic, no timestamp)
    // Hash all pubkeys in fixed order: x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc
    let offer_provenance = {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&payload.x25519_public);
        hasher.update(&payload.p384_public);
        hasher.update(&payload.secp256k1_public);
        hasher.update(&payload.p256_public);
        hasher.update(&payload.frodo976_public);
        hasher.update(&payload.ntru701_public);
        hasher.update(&payload.mceliece_public);
        hasher.update(&payload.hqc256_public);
        *hasher.finalize().as_bytes()
    };

    Ok((signed, offer_provenance))
}

/// Parse and verify a VSF ClutchOffer message.
///
/// Verifies:
/// 1. VSF format and magic bytes
/// 2. Ed25519 signature (header-level)
/// 3. conversation_token matches expected token for our conversation
///
/// Returns (payload, sender_pubkey, offer_provenance, conversation_token)
/// - offer_provenance: Deterministic hash of the offer's public keys (no timestamp)
/// - conversation_token: Privacy-preserving smear_hash of sorted participant identity seeds
pub fn parse_clutch_offer_vsf(
    vsf_bytes: &[u8],
    expected_conversation_token: &[u8; 32],
) -> Result<(ClutchOfferPayload, [u8; 32], [u8; 32], [u8; 32]), String> {
    // Verify signature first
    if !vsf::verification::verify_file_signature(vsf_bytes)? {
        return Err("Invalid signature on ClutchOffer".to_string());
    }

    // Extract signer pubkey
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    // Parse header to get section start position
    use vsf::file_format::VsfHeader;
    let (_header, header_end) =
        VsfHeader::decode(vsf_bytes).map_err(|e| format!("Failed to parse header: {}", e))?;

    // offer_provenance will be computed from keys after parsing (deterministic, no timestamp)

    // Parse section with schema
    let section_bytes = &vsf_bytes[header_end..];
    let schema = clutch_offer_schema();
    let builder = SectionBuilder::parse(schema, section_bytes)
        .map_err(|e| format!("Parse clutch_offer: {}", e))?;

    // Convert to VsfField vec for backwards compatibility with extraction code
    use vsf::file_format::VsfField;
    let mut fields: Vec<VsfField> = Vec::new();
    for field_name in ["tok", "pubkey", "pubkeys"] {
        if let Ok(values) = builder.get(field_name) {
            fields.push(VsfField {
                name: field_name.to_string(),
                values: values.clone(),
            });
        }
    }

    // Extract conversation_token (single value field)
    let conversation_token: [u8; 32] = fields
        .iter()
        .find(|f| f.name == "tok")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hg(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        })
        .ok_or("Missing or invalid conversation_token")?;

    // Verify conversation_token matches expected
    if &conversation_token != expected_conversation_token {
        return Err("ClutchOffer conversation_token mismatch".to_string());
    }

    // Extract pubkeys from multi-value "pubkeys" field (kx, kp, kk, kp, kf, kn, kl, kh order)
    // Also support legacy individual fields for backwards compatibility
    let (
        x25519_public,
        p384_public,
        secp256k1_public,
        p256_public,
        frodo976_public,
        ntru701_public,
        mceliece_public,
        hqc256_public,
    ) = if let Some(pk_field) = fields.iter().find(|f| f.name == "pubkeys") {
        // New format: multi-value field
        let mut x25519 = [0u8; 32];
        let mut p384 = Vec::new();
        let mut secp256k1 = Vec::new();
        let mut p256 = Vec::new();
        let mut frodo = Vec::new();
        let mut ntru = Vec::new();
        let mut mceliece = Vec::new();
        let mut hqc = Vec::new();
        let mut kp_index = 0;
        for v in &pk_field.values {
            match v {
                VsfType::kx(data) if data.len() == 32 => {
                    x25519.copy_from_slice(data);
                }
                VsfType::kp(data) => {
                    // P-384 (97B) comes before P-256 (65B) in order
                    if kp_index == 0 {
                        p384 = data.clone();
                    } else {
                        p256 = data.clone();
                    }
                    kp_index += 1;
                }
                VsfType::kk(data) => secp256k1 = data.clone(),
                VsfType::kf(data) => frodo = data.clone(),
                VsfType::kn(data) => ntru = data.clone(),
                VsfType::kl(data) => mceliece = data.clone(),
                VsfType::kh(data) => hqc = data.clone(),
                _ => {}
            }
        }
        (x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc)
    } else {
        // Legacy format: individual named fields
        let legacy_fields: Vec<(String, VsfType)> = fields
            .iter()
            .flat_map(|f| f.values.first().map(|v| (f.name.clone(), v.clone())))
            .collect();
        (
            extract_kx(&legacy_fields, "x25519")?,
            extract_kp(&legacy_fields, "p384")?,
            extract_kk(&legacy_fields, "secp256k1")?,
            extract_kp(&legacy_fields, "p256")?,
            extract_kf(&legacy_fields, "frodo")?,
            extract_kn(&legacy_fields, "ntru")?,
            extract_kl(&legacy_fields, "mceliece")?,
            extract_kh(&legacy_fields, "hqc")?,
        )
    };

    let payload = ClutchOfferPayload {
        x25519_public,
        p384_public,
        secp256k1_public,
        p256_public,
        frodo976_public,
        ntru701_public,
        mceliece_public,
        hqc256_public,
    };

    // Compute offer_provenance from keys (deterministic, no timestamp)
    // Hash all pubkeys in fixed order: x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc
    let offer_provenance = {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&payload.x25519_public);
        hasher.update(&payload.p384_public);
        hasher.update(&payload.secp256k1_public);
        hasher.update(&payload.p256_public);
        hasher.update(&payload.frodo976_public);
        hasher.update(&payload.ntru701_public);
        hasher.update(&payload.mceliece_public);
        hasher.update(&payload.hqc256_public);
        *hasher.finalize().as_bytes()
    };

    #[cfg(feature = "development")]
    {
        let prov_hex: String = offer_provenance
            .iter()
            .take(8)
            .map(|b| format!("{:02x}", b))
            .collect();
        crate::log(&format!(
            "CLUTCH: Received offer ({} bytes) offer_provenance={}... (key-based)",
            vsf_bytes.len(),
            prov_hex
        ));
        crate::log(&format!(
            "CLUTCH: Offer pubkeys (X25519: {}B, P-384: {}B, secp256k1: {}B, P-256: {}B, Frodo: {}B, NTRU: {}B, McEliece: {}B, HQC: {}B)",
            payload.x25519_public.len(),
            payload.p384_public.len(),
            payload.secp256k1_public.len(),
            payload.p256_public.len(),
            payload.frodo976_public.len(),
            payload.ntru701_public.len(),
            payload.mceliece_public.len(),
            payload.hqc256_public.len()
        ));
        crate::log(&format!(
            "CLUTCH: Parsed offer HQC pub[..8]={}",
            hex::encode(&payload.hqc256_public[..8])
        ));
    }

    Ok((payload, sender_pubkey, offer_provenance, conversation_token))
}

/// Build a signed VSF ClutchKemResponse message (~31KB).
///
/// Uses VSF v() wrapped type for KEM ciphertexts:
/// - v(b'f', ...): FrodoKEM ciphertext
/// - v(b'n', ...): NTRU ciphertext
/// - v(b'l', ...): Classic McEliece ciphertext
/// - v(b'c', ...): HQC ciphertext
///
/// The ceremony_id is deterministic - both parties compute the same value.
///
/// conversation_token: Privacy-preserving smear_hash of sorted participant identity seeds.
pub fn build_clutch_kem_response_vsf(
    conversation_token: &[u8; 32],
    ceremony_id: &[u8; 32],
    payload: &ClutchKemResponsePayload,
    device_pubkey: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<Vec<u8>, String> {
    use vsf::file_format::VsfSection;
    use vsf::VsfBuilder;

    // Build section with multi-value fields (matches keypairs.vsf format)
    let mut section = VsfSection::new("clutch_kem_response");
    section.add_field("tok", VsfType::hg(conversation_token.to_vec()));
    // Target HQC pub prefix for stale KEM response detection
    section.add_field(
        "target_hqc",
        VsfType::hb(payload.target_hqc_pub_prefix.to_vec()),
    );
    // PQC KEM ciphertexts as multi-value field (vf, vn, vl, vc order matches offer pubkeys)
    section.add_field_multi(
        "ciphertexts",
        vec![
            VsfType::v(b'f', payload.frodo976_ciphertext.clone()),
            VsfType::v(b'n', payload.ntru701_ciphertext.clone()),
            VsfType::v(b'l', payload.mceliece_ciphertext.clone()),
            VsfType::v(b'c', payload.hqc256_ciphertext.clone()),
        ],
    );
    // EC ephemeral pubkeys as multi-value field (kx, kp, kk, kp order matches offer pubkeys)
    section.add_field_multi(
        "ephemerals",
        vec![
            VsfType::kx(payload.x25519_ephemeral.to_vec()),
            VsfType::kp(payload.p384_ephemeral.clone()),
            VsfType::kk(payload.secp256k1_ephemeral.clone()),
            VsfType::kp(payload.p256_ephemeral.clone()),
        ],
    );

    // Build unsigned VSF with signature placeholder
    let unsigned = VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .provenance_hash(*ceremony_id)
        .signature_ed25519(*device_pubkey, [0u8; 64])
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("Failed to build ClutchKemResponse VSF: {}", e))?;

    // Sign the file
    vsf::verification::sign_file(unsigned, device_secret)
}

/// Parse and verify a VSF ClutchKemResponse message.
///
/// Verifies:
/// 1. VSF format and magic bytes
/// 2. Ed25519 signature (header-level)
/// 3. conversation_token matches expected token for our conversation
///
/// Returns (payload, sender_pubkey, ceremony_id, conversation_token)
/// conversation_token is the privacy-preserving smear_hash of sorted participant identity seeds.
pub fn parse_clutch_kem_response_vsf(
    vsf_bytes: &[u8],
    expected_conversation_token: &[u8; 32],
) -> Result<(ClutchKemResponsePayload, [u8; 32], [u8; 32], [u8; 32]), String> {
    // Verify signature first
    if !vsf::verification::verify_file_signature(vsf_bytes)? {
        return Err("Invalid signature on ClutchKemResponse".to_string());
    }

    // Extract signer pubkey
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    // Parse header for ceremony_id
    use vsf::file_format::VsfHeader;
    let (header, header_end) =
        VsfHeader::decode(vsf_bytes).map_err(|e| format!("Failed to parse header: {}", e))?;

    let ceremony_id = extract_header_provenance(&header)?;

    // Parse section with schema
    let section_bytes = &vsf_bytes[header_end..];
    let schema = clutch_kem_response_schema();
    let builder = SectionBuilder::parse(schema, section_bytes)
        .map_err(|e| format!("Parse clutch_kem_response: {}", e))?;

    // Convert to VsfField vec for backwards compatibility with extraction code
    use vsf::file_format::VsfField;
    let mut fields: Vec<VsfField> = Vec::new();
    for field_name in ["tok", "target_hqc", "ciphertext", "ciphertexts", "ephemerals"] {
        if let Ok(values) = builder.get(field_name) {
            fields.push(VsfField {
                name: field_name.to_string(),
                values: values.clone(),
            });
        }
    }

    // Extract conversation_token (single value field)
    let conversation_token: [u8; 32] = fields
        .iter()
        .find(|f| f.name == "tok")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hg(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        })
        .ok_or("Missing or invalid conversation_token")?;

    // Verify conversation_token matches expected
    if &conversation_token != expected_conversation_token {
        return Err("ClutchKemResponse conversation_token mismatch".to_string());
    }

    // Extract target HQC pub prefix for stale detection
    let target_hqc_pub_prefix: [u8; 8] = fields
        .iter()
        .find(|f| f.name == "target_hqc")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hb(bytes) if bytes.len() >= 8 => {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&bytes[..8]);
                Some(arr)
            }
            _ => None,
        })
        .unwrap_or([0u8; 8]);

    // Extract ciphertexts from multi-value "ciphertexts" field (vf, vn, vl, vc order)
    // Also support legacy individual fields for backwards compatibility
    let (frodo976_ciphertext, ntru701_ciphertext, mceliece_ciphertext, hqc256_ciphertext) =
        if let Some(ct_field) = fields.iter().find(|f| f.name == "ciphertexts") {
            // New format: multi-value field
            let mut frodo = Vec::new();
            let mut ntru = Vec::new();
            let mut mceliece = Vec::new();
            let mut hqc = Vec::new();
            for v in &ct_field.values {
                match v {
                    VsfType::v(b'f', data) => frodo = data.clone(),
                    VsfType::v(b'n', data) => ntru = data.clone(),
                    VsfType::v(b'l', data) => mceliece = data.clone(),
                    VsfType::v(b'c', data) => hqc = data.clone(),
                    _ => {}
                }
            }
            (frodo, ntru, mceliece, hqc)
        } else {
            // Legacy format: individual named fields
            let legacy_fields: Vec<(String, VsfType)> = fields
                .iter()
                .flat_map(|f| f.values.first().map(|v| (f.name.clone(), v.clone())))
                .collect();
            (
                extract_v(&legacy_fields, "frodo_ct", b'f').unwrap_or_default(),
                extract_v(&legacy_fields, "ntru_ct", b'n').unwrap_or_default(),
                extract_v(&legacy_fields, "mceliece_ct", b'l').unwrap_or_default(),
                extract_v(&legacy_fields, "hqc_ct", b'c').unwrap_or_default(),
            )
        };

    // Extract EC ephemeral pubkeys from multi-value "ephemerals" field (kx, kp, kk, kp order)
    // Also support legacy individual fields for backwards compatibility
    let (x25519_ephemeral, p384_ephemeral, secp256k1_ephemeral, p256_ephemeral) =
        if let Some(eph_field) = fields.iter().find(|f| f.name == "ephemerals") {
            // New format: multi-value field
            let mut x25519 = [0u8; 32];
            let mut p384 = Vec::new();
            let mut secp256k1 = Vec::new();
            let mut p256 = Vec::new();
            let mut kp_index = 0;
            for v in &eph_field.values {
                match v {
                    VsfType::kx(data) if data.len() == 32 => {
                        x25519.copy_from_slice(data);
                    }
                    VsfType::kp(data) => {
                        // P-384 (97B) comes before P-256 (65B) in order
                        if kp_index == 0 {
                            p384 = data.clone();
                        } else {
                            p256 = data.clone();
                        }
                        kp_index += 1;
                    }
                    VsfType::kk(data) => secp256k1 = data.clone(),
                    _ => {}
                }
            }
            (x25519, p384, secp256k1, p256)
        } else {
            // Legacy format: individual named fields
            let x25519 = fields
                .iter()
                .find(|f| f.name == "x25519_eph")
                .and_then(|f| f.values.first())
                .and_then(|v| match v {
                    VsfType::kx(data) if data.len() == 32 => {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(data);
                        Some(arr)
                    }
                    _ => None,
                })
                .unwrap_or([0u8; 32]);
            let p384 = fields
                .iter()
                .find(|f| f.name == "p384_eph")
                .and_then(|f| f.values.first())
                .and_then(|v| match v {
                    VsfType::kp(data) => Some(data.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let secp256k1 = fields
                .iter()
                .find(|f| f.name == "secp256k1_eph")
                .and_then(|f| f.values.first())
                .and_then(|v| match v {
                    VsfType::kk(data) => Some(data.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let p256 = fields
                .iter()
                .find(|f| f.name == "p256_eph")
                .and_then(|f| f.values.first())
                .and_then(|v| match v {
                    VsfType::kp(data) => Some(data.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            (x25519, p384, secp256k1, p256)
        };

    let payload = ClutchKemResponsePayload {
        frodo976_ciphertext,
        ntru701_ciphertext,
        mceliece_ciphertext,
        hqc256_ciphertext,
        target_hqc_pub_prefix,
        x25519_ephemeral,
        p384_ephemeral,
        secp256k1_ephemeral,
        p256_ephemeral,
    };

    #[cfg(feature = "development")]
    {
        let hp_hex: String = ceremony_id
            .iter()
            .take(8)
            .map(|b| format!("{:02x}", b))
            .collect();
        crate::log(&format!(
            "CLUTCH: Received KEM response ({} bytes) ceremony_id={}...",
            vsf_bytes.len(),
            hp_hex
        ));
        crate::log(&format!(
            "CLUTCH: KEM ciphertexts (Frodo: {}B, NTRU: {}B, McEliece: {}B, HQC: {}B)",
            payload.frodo976_ciphertext.len(),
            payload.ntru701_ciphertext.len(),
            payload.mceliece_ciphertext.len(),
            payload.hqc256_ciphertext.len()
        ));
        crate::log(&format!(
            "CLUTCH: Parsed KEM response HQC ct[..8]={}, EC ephemerals: X25519 {}B, P384 {}B",
            hex::encode(&payload.hqc256_ciphertext[..8]),
            payload.x25519_ephemeral.len(),
            payload.p384_ephemeral.len()
        ));
    }

    Ok((payload, sender_pubkey, ceremony_id, conversation_token))
}

/// Parse and verify a VSF ClutchOffer message WITHOUT recipient check.
///
/// This variant is used by the TCP receiver which doesn't know our conversation_token.
/// The caller (app.rs) is responsible for verifying the message is addressed to them.
///
/// Verifies:
/// 1. VSF format and magic bytes
/// 2. Ed25519 signature (header-level)
///
/// Returns (payload, sender_pubkey, offer_provenance, conversation_token)
/// - offer_provenance: Deterministic hash of the offer's public keys (no timestamp)
pub fn parse_clutch_offer_vsf_without_recipient_check(
    vsf_bytes: &[u8],
) -> Result<(ClutchOfferPayload, [u8; 32], [u8; 32], [u8; 32]), String> {
    // Verify signature first
    if !vsf::verification::verify_file_signature(vsf_bytes)? {
        return Err("Invalid signature on ClutchOffer".to_string());
    }

    // Extract signer pubkey
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    // Parse header to get section start position
    use vsf::file_format::VsfHeader;
    let (_header, header_end) =
        VsfHeader::decode(vsf_bytes).map_err(|e| format!("Failed to parse header: {}", e))?;

    // offer_provenance will be computed from keys after parsing (deterministic, no timestamp)

    // Parse section with schema
    let section_bytes = &vsf_bytes[header_end..];
    let schema = clutch_offer_schema();
    let builder = SectionBuilder::parse(schema, section_bytes)
        .map_err(|e| format!("Parse clutch_offer: {}", e))?;

    // Convert to VsfField vec for backwards compatibility with extraction code
    use vsf::file_format::VsfField;
    let mut fields: Vec<VsfField> = Vec::new();
    for field_name in ["tok", "pubkey", "pubkeys"] {
        if let Ok(values) = builder.get(field_name) {
            fields.push(VsfField {
                name: field_name.to_string(),
                values: values.clone(),
            });
        }
    }

    // Extract conversation_token (NO recipient check - caller verifies)
    let conversation_token: [u8; 32] = fields
        .iter()
        .find(|f| f.name == "tok")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hg(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        })
        .ok_or("Missing or invalid conversation_token")?;

    // Extract pubkeys from multi-value "pubkeys" field (kx, kp, kk, kp, kf, kn, kl, kh order)
    // Also support legacy individual fields for backwards compatibility
    let (
        x25519_public,
        p384_public,
        secp256k1_public,
        p256_public,
        frodo976_public,
        ntru701_public,
        mceliece_public,
        hqc256_public,
    ) = if let Some(pk_field) = fields.iter().find(|f| f.name == "pubkeys") {
        // New format: multi-value field
        let mut x25519 = [0u8; 32];
        let mut p384 = Vec::new();
        let mut secp256k1 = Vec::new();
        let mut p256 = Vec::new();
        let mut frodo = Vec::new();
        let mut ntru = Vec::new();
        let mut mceliece = Vec::new();
        let mut hqc = Vec::new();
        let mut kp_index = 0;
        for v in &pk_field.values {
            match v {
                VsfType::kx(data) if data.len() == 32 => {
                    x25519.copy_from_slice(data);
                }
                VsfType::kp(data) => {
                    // P-384 (97B) comes before P-256 (65B) in order
                    if kp_index == 0 {
                        p384 = data.clone();
                    } else {
                        p256 = data.clone();
                    }
                    kp_index += 1;
                }
                VsfType::kk(data) => secp256k1 = data.clone(),
                VsfType::kf(data) => frodo = data.clone(),
                VsfType::kn(data) => ntru = data.clone(),
                VsfType::kl(data) => mceliece = data.clone(),
                VsfType::kh(data) => hqc = data.clone(),
                _ => {}
            }
        }
        (x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc)
    } else {
        // Legacy format: individual named fields
        let legacy_fields: Vec<(String, VsfType)> = fields
            .iter()
            .flat_map(|f| f.values.first().map(|v| (f.name.clone(), v.clone())))
            .collect();
        (
            extract_kx(&legacy_fields, "x25519")?,
            extract_kp(&legacy_fields, "p384")?,
            extract_kk(&legacy_fields, "secp256k1")?,
            extract_kp(&legacy_fields, "p256")?,
            extract_kf(&legacy_fields, "frodo")?,
            extract_kn(&legacy_fields, "ntru")?,
            extract_kl(&legacy_fields, "mceliece")?,
            extract_kh(&legacy_fields, "hqc")?,
        )
    };

    let payload = ClutchOfferPayload {
        x25519_public,
        p384_public,
        secp256k1_public,
        p256_public,
        frodo976_public,
        ntru701_public,
        mceliece_public,
        hqc256_public,
    };

    // Compute offer_provenance from keys (deterministic, no timestamp)
    // Hash all pubkeys in fixed order: x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc
    let offer_provenance = {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&payload.x25519_public);
        hasher.update(&payload.p384_public);
        hasher.update(&payload.secp256k1_public);
        hasher.update(&payload.p256_public);
        hasher.update(&payload.frodo976_public);
        hasher.update(&payload.ntru701_public);
        hasher.update(&payload.mceliece_public);
        hasher.update(&payload.hqc256_public);
        *hasher.finalize().as_bytes()
    };

    #[cfg(feature = "development")]
    crate::log(&format!(
        "CLUTCH: Parsed offer (no recipient check) HQC pub[..8]={} provenance={}...",
        hex::encode(&payload.hqc256_public[..8]),
        hex::encode(&offer_provenance[..8])
    ));

    Ok((payload, sender_pubkey, offer_provenance, conversation_token))
}

/// Parse and verify a VSF ClutchKemResponse message WITHOUT recipient check.
///
/// This variant is used by the TCP receiver which doesn't know our conversation_token.
/// The caller (app.rs) is responsible for verifying the message is addressed to them.
///
/// Verifies:
/// 1. VSF format and magic bytes
/// 2. Ed25519 signature (header-level)
///
/// Returns (payload, sender_pubkey, ceremony_id, conversation_token)
pub fn parse_clutch_kem_response_vsf_without_recipient_check(
    vsf_bytes: &[u8],
) -> Result<(ClutchKemResponsePayload, [u8; 32], [u8; 32], [u8; 32]), String> {
    // Verify signature first
    if !vsf::verification::verify_file_signature(vsf_bytes)? {
        return Err("Invalid signature on ClutchKemResponse".to_string());
    }

    // Extract signer pubkey
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    // Parse header for ceremony_id
    use vsf::file_format::VsfHeader;
    let (header, header_end) =
        VsfHeader::decode(vsf_bytes).map_err(|e| format!("Failed to parse header: {}", e))?;

    let ceremony_id = extract_header_provenance(&header)?;

    // Parse section with schema
    let section_bytes = &vsf_bytes[header_end..];
    let schema = clutch_kem_response_schema();
    let builder = SectionBuilder::parse(schema, section_bytes)
        .map_err(|e| format!("Parse clutch_kem_response: {}", e))?;

    // Convert to VsfField vec for backwards compatibility with extraction code
    use vsf::file_format::VsfField;
    let mut fields: Vec<VsfField> = Vec::new();
    for field_name in ["tok", "target_hqc", "ciphertext", "ciphertexts", "ephemerals"] {
        if let Ok(values) = builder.get(field_name) {
            fields.push(VsfField {
                name: field_name.to_string(),
                values: values.clone(),
            });
        }
    }

    // Extract conversation_token (NO recipient check - caller verifies)
    let conversation_token: [u8; 32] = fields
        .iter()
        .find(|f| f.name == "tok")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hg(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        })
        .ok_or("Missing or invalid conversation_token")?;

    // Extract target HQC pub prefix for stale detection
    let target_hqc_pub_prefix: [u8; 8] = fields
        .iter()
        .find(|f| f.name == "target_hqc")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hb(bytes) if bytes.len() >= 8 => {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&bytes[..8]);
                Some(arr)
            }
            _ => None,
        })
        .unwrap_or([0u8; 8]);

    // Extract ciphertexts from multi-value "ciphertexts" field (vf, vn, vl, vc order)
    // Also support legacy individual fields for backwards compatibility
    let (frodo976_ciphertext, ntru701_ciphertext, mceliece_ciphertext, hqc256_ciphertext) =
        if let Some(ct_field) = fields.iter().find(|f| f.name == "ciphertexts") {
            // New format: multi-value field
            let mut frodo = Vec::new();
            let mut ntru = Vec::new();
            let mut mceliece = Vec::new();
            let mut hqc = Vec::new();
            for v in &ct_field.values {
                match v {
                    VsfType::v(b'f', data) => frodo = data.clone(),
                    VsfType::v(b'n', data) => ntru = data.clone(),
                    VsfType::v(b'l', data) => mceliece = data.clone(),
                    VsfType::v(b'c', data) => hqc = data.clone(),
                    _ => {}
                }
            }
            (frodo, ntru, mceliece, hqc)
        } else {
            // Legacy format: individual named fields
            let legacy_fields: Vec<(String, VsfType)> = fields
                .iter()
                .flat_map(|f| f.values.first().map(|v| (f.name.clone(), v.clone())))
                .collect();
            (
                extract_v(&legacy_fields, "frodo_ct", b'f').unwrap_or_default(),
                extract_v(&legacy_fields, "ntru_ct", b'n').unwrap_or_default(),
                extract_v(&legacy_fields, "mceliece_ct", b'l').unwrap_or_default(),
                extract_v(&legacy_fields, "hqc_ct", b'c').unwrap_or_default(),
            )
        };

    // Extract EC ephemeral pubkeys from multi-value "ephemerals" field (kx, kp, kk, kp order)
    // Also support legacy individual fields for backwards compatibility
    let (x25519_ephemeral, p384_ephemeral, secp256k1_ephemeral, p256_ephemeral) =
        if let Some(eph_field) = fields.iter().find(|f| f.name == "ephemerals") {
            // New format: multi-value field
            let mut x25519 = [0u8; 32];
            let mut p384 = Vec::new();
            let mut secp256k1 = Vec::new();
            let mut p256 = Vec::new();
            let mut kp_index = 0;
            for v in &eph_field.values {
                match v {
                    VsfType::kx(data) if data.len() == 32 => {
                        x25519.copy_from_slice(data);
                    }
                    VsfType::kp(data) => {
                        // P-384 (97B) comes before P-256 (65B) in order
                        if kp_index == 0 {
                            p384 = data.clone();
                        } else {
                            p256 = data.clone();
                        }
                        kp_index += 1;
                    }
                    VsfType::kk(data) => secp256k1 = data.clone(),
                    _ => {}
                }
            }
            (x25519, p384, secp256k1, p256)
        } else {
            // Legacy format: individual named fields
            let x25519 = fields
                .iter()
                .find(|f| f.name == "x25519_eph")
                .and_then(|f| f.values.first())
                .and_then(|v| match v {
                    VsfType::kx(data) if data.len() == 32 => {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(data);
                        Some(arr)
                    }
                    _ => None,
                })
                .unwrap_or([0u8; 32]);
            let p384 = fields
                .iter()
                .find(|f| f.name == "p384_eph")
                .and_then(|f| f.values.first())
                .and_then(|v| match v {
                    VsfType::kp(data) => Some(data.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let secp256k1 = fields
                .iter()
                .find(|f| f.name == "secp256k1_eph")
                .and_then(|f| f.values.first())
                .and_then(|v| match v {
                    VsfType::kk(data) => Some(data.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let p256 = fields
                .iter()
                .find(|f| f.name == "p256_eph")
                .and_then(|f| f.values.first())
                .and_then(|v| match v {
                    VsfType::kp(data) => Some(data.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            (x25519, p384, secp256k1, p256)
        };

    let payload = ClutchKemResponsePayload {
        frodo976_ciphertext,
        ntru701_ciphertext,
        mceliece_ciphertext,
        hqc256_ciphertext,
        target_hqc_pub_prefix,
        x25519_ephemeral,
        p384_ephemeral,
        secp256k1_ephemeral,
        p256_ephemeral,
    };

    #[cfg(feature = "development")]
    crate::log(&format!(
        "CLUTCH: Parsed KEM response (no recipient check) HQC ct[..8]={} target_hqc[..8]={} EC ephemerals present",
        hex::encode(&payload.hqc256_ciphertext[..8]),
        hex::encode(&payload.target_hqc_pub_prefix)
    ));

    Ok((payload, sender_pubkey, ceremony_id, conversation_token))
}

// Helper functions for extracting VSF key types

fn extract_kx(fields: &[(String, VsfType)], key: &str) -> Result<[u8; 32], String> {
    match get_field(fields, key) {
        Some(VsfType::kx(bytes)) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(bytes);
            Ok(arr)
        }
        _ => Err(format!("Missing or invalid kx field: {}", key)),
    }
}

fn extract_kp(fields: &[(String, VsfType)], key: &str) -> Result<Vec<u8>, String> {
    match get_field(fields, key) {
        Some(VsfType::kp(bytes)) => Ok(bytes.clone()),
        _ => Err(format!("Missing or invalid kp field: {}", key)),
    }
}

fn extract_kk(fields: &[(String, VsfType)], key: &str) -> Result<Vec<u8>, String> {
    match get_field(fields, key) {
        Some(VsfType::kk(bytes)) => Ok(bytes.clone()),
        _ => Err(format!("Missing or invalid kk field: {}", key)),
    }
}

fn extract_kf(fields: &[(String, VsfType)], key: &str) -> Result<Vec<u8>, String> {
    match get_field(fields, key) {
        Some(VsfType::kf(bytes)) => Ok(bytes.clone()),
        _ => Err(format!("Missing or invalid kf field: {}", key)),
    }
}

fn extract_kn(fields: &[(String, VsfType)], key: &str) -> Result<Vec<u8>, String> {
    match get_field(fields, key) {
        Some(VsfType::kn(bytes)) => Ok(bytes.clone()),
        _ => Err(format!("Missing or invalid kn field: {}", key)),
    }
}

fn extract_kl(fields: &[(String, VsfType)], key: &str) -> Result<Vec<u8>, String> {
    match get_field(fields, key) {
        Some(VsfType::kl(bytes)) => Ok(bytes.clone()),
        _ => Err(format!("Missing or invalid kl field: {}", key)),
    }
}

fn extract_kh(fields: &[(String, VsfType)], key: &str) -> Result<Vec<u8>, String> {
    match get_field(fields, key) {
        Some(VsfType::kh(bytes)) => Ok(bytes.clone()),
        _ => Err(format!("Missing or invalid kh field: {}", key)),
    }
}

fn extract_v(fields: &[(String, VsfType)], key: &str, expected_tag: u8) -> Result<Vec<u8>, String> {
    match get_field(fields, key) {
        Some(VsfType::v(tag, bytes)) if *tag == expected_tag => Ok(bytes.clone()),
        Some(VsfType::v(tag, _)) => Err(format!(
            "Wrong tag for {}: expected '{}', got '{}'",
            key, expected_tag as char, *tag as char
        )),
        _ => Err(format!("Missing or invalid v field: {}", key)),
    }
}

// =============================================================================
// CLUTCH COMPLETE (Proof Exchange)
// =============================================================================

/// Build a signed VSF ClutchComplete message (~200 bytes).
///
/// Contains the eggs_proof hash for verification. Both parties send this
/// after computing their eggs, and verify the peer's proof matches.
///
/// conversation_token: Privacy-preserving smear_hash of sorted participant identity seeds.
pub fn build_clutch_complete_vsf(
    conversation_token: &[u8; 32],
    ceremony_id: &[u8; 32],
    payload: &ClutchCompletePayload,
    device_pubkey: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<Vec<u8>, String> {
    use vsf::VsfBuilder;

    // Build unsigned VSF with signature placeholder
    let unsigned = VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .provenance_hash(*ceremony_id)
        .signature_ed25519(*device_pubkey, [0u8; 64])
        .add_section(
            "clutch_complete",
            vec![
                ("tok".to_string(), VsfType::hg(conversation_token.to_vec())),
                (
                    "eggs_proof".to_string(),
                    VsfType::hg(payload.eggs_proof.to_vec()),
                ),
            ],
        )
        .build()
        .map_err(|e| format!("Failed to build ClutchComplete VSF: {}", e))?;

    // Sign the file
    vsf::verification::sign_file(unsigned, device_secret)
}

/// Parse and verify a VSF ClutchComplete message.
///
/// Verifies:
/// 1. VSF format and magic bytes
/// 2. Ed25519 signature (header-level)
/// 3. conversation_token matches expected token for our conversation
///
/// Returns (payload, sender_pubkey, ceremony_id, conversation_token)
pub fn parse_clutch_complete_vsf(
    vsf_bytes: &[u8],
    expected_conversation_token: &[u8; 32],
) -> Result<(ClutchCompletePayload, [u8; 32], [u8; 32], [u8; 32]), String> {
    // Verify signature first
    if !vsf::verification::verify_file_signature(vsf_bytes)? {
        return Err("Invalid signature on ClutchComplete".to_string());
    }

    // Extract signer pubkey
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    // Parse header for ceremony_id
    use vsf::file_format::VsfHeader;
    let (header, header_end) =
        VsfHeader::decode(vsf_bytes).map_err(|e| format!("Failed to parse header: {}", e))?;

    let ceremony_id = extract_header_provenance(&header)?;

    // Parse section with schema validation
    let section_bytes = &vsf_bytes[header_end..];
    let schema = clutch_complete_schema();
    let builder = SectionBuilder::parse(schema, section_bytes)
        .map_err(|e| format!("Parse clutch_complete: {}", e))?;

    // Extract conversation_token
    let tok_values = builder.get("tok")
        .map_err(|e| format!("No tok field: {}", e))?;
    let conversation_token = match tok_values.first() {
        Some(VsfType::hg(hash)) if hash.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(hash);
            arr
        }
        _ => return Err("Invalid tok field type".to_string()),
    };

    // Verify conversation_token matches expected
    if &conversation_token != expected_conversation_token {
        return Err("ClutchComplete conversation_token mismatch".to_string());
    }

    // Extract eggs_proof
    let eggs_proof_values = builder.get("eggs_proof")
        .map_err(|e| format!("No eggs_proof field: {}", e))?;
    let eggs_proof = match eggs_proof_values.first() {
        Some(VsfType::hg(hash)) if hash.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(hash);
            arr
        }
        _ => return Err("Invalid eggs_proof field type".to_string()),
    };

    let payload = ClutchCompletePayload { eggs_proof };

    #[cfg(feature = "development")]
    {
        let id_hex: String = ceremony_id
            .iter()
            .take(8)
            .map(|b| format!("{:02x}", b))
            .collect();
        crate::log(&format!(
            "CLUTCH: Received complete proof ({} bytes) ceremony_id={}... proof={}...",
            vsf_bytes.len(),
            id_hex,
            hex::encode(&payload.eggs_proof[..8])
        ));
    }

    Ok((payload, sender_pubkey, ceremony_id, conversation_token))
}

/// Parse and verify a VSF ClutchComplete message WITHOUT recipient check.
///
/// This variant is used by the TCP receiver which doesn't know our conversation_token.
/// The caller (app.rs) is responsible for verifying the message is addressed to them.
pub fn parse_clutch_complete_vsf_without_recipient_check(
    vsf_bytes: &[u8],
) -> Result<(ClutchCompletePayload, [u8; 32], [u8; 32], [u8; 32]), String> {
    // Verify signature first
    if !vsf::verification::verify_file_signature(vsf_bytes)? {
        return Err("Invalid signature on ClutchComplete".to_string());
    }

    // Extract signer pubkey
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    // Parse header for ceremony_id
    use vsf::file_format::VsfHeader;
    let (header, header_end) =
        VsfHeader::decode(vsf_bytes).map_err(|e| format!("Failed to parse header: {}", e))?;

    let ceremony_id = extract_header_provenance(&header)?;

    // Parse section with schema validation
    let section_bytes = &vsf_bytes[header_end..];
    let schema = clutch_complete_schema();
    let builder = SectionBuilder::parse(schema, section_bytes)
        .map_err(|e| format!("Parse clutch_complete: {}", e))?;

    // Extract conversation_token (NO recipient check - caller verifies)
    let tok_values = builder.get("tok")
        .map_err(|e| format!("No tok field: {}", e))?;
    let conversation_token = match tok_values.first() {
        Some(VsfType::hg(hash)) if hash.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(hash);
            arr
        }
        _ => return Err("Invalid tok field type".to_string()),
    };

    // Extract eggs_proof
    let eggs_proof_values = builder.get("eggs_proof")
        .map_err(|e| format!("No eggs_proof field: {}", e))?;
    let eggs_proof = match eggs_proof_values.first() {
        Some(VsfType::hg(hash)) if hash.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(hash);
            arr
        }
        _ => return Err("Invalid eggs_proof field type".to_string()),
    };

    let payload = ClutchCompletePayload { eggs_proof };

    #[cfg(feature = "development")]
    crate::log(&format!(
        "CLUTCH: Parsed complete proof (no recipient check) proof={}...",
        hex::encode(&payload.eggs_proof[..8])
    ));

    Ok((payload, sender_pubkey, ceremony_id, conversation_token))
}
