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
    /// Simplified header-only format: RÅ< z4 y2 ef6[timestamp] hp[provenance] ke[pubkey] ge[signature] n1 (ping) >
    ///
    /// - provenance_hash = BLAKE3(sender_pubkey || timestamp_nanos)
    /// - ke = sender's Ed25519 public key (for signature verification)
    /// - ge = signature of provenance_hash
    ///
    /// Note: Avatar is fetched by handle, not exchanged in ping/pong. Storage key = BLAKE3(BLAKE3(handle) || "avatar")
    StatusPing {
        timestamp: i64,              // Eagle time oscillations (i64)
        sender_pubkey: DevicePubkey, // Who is pinging (for response routing)
        provenance_hash: [u8; 32],   // BLAKE3(sender_pubkey || timestamp_oscillations)
        signature: [u8; 64],         // Ed25519 signature of provenance_hash
    },
    /// P2P status pong - "yes I'm online"
    ///
    /// Format: RÅ< z4 y2 ef6[timestamp] hp[SAME provenance] ke[pubkey] ge[signature] n1 (pong) > [pong (sync_count: N) (sync_0_tok: hb) (sync_0_ef6: f6) ...]
    ///
    /// - Echoes same provenance_hash from ping (proves we saw it)
    /// - ke = responder's Ed25519 public key (for signature verification)
    /// - ge = signature of provenance_hash (proves we processed it)
    /// - sync records: Per-conversation last_received_ef6 for efficient resync Peer can retransmit everything after that timestamp
    ///
    /// Note: Avatar is fetched by handle, not exchanged in ping/pong. Storage key = BLAKE3(BLAKE3(handle) || "avatar")
    StatusPong {
        timestamp: i64,                 // Responder's current Eagle time oscillations (i64)
        responder_pubkey: DevicePubkey, // Who is responding
        provenance_hash: [u8; 32],      // Same hash from ping (proves we received it)
        signature: [u8; 64],            // Ed25519 signature of provenance_hash
        /// Per-conversation sync records: (conversation_token, last_received_osc) Tells peer: "For this conversation, your last message I received was at time X" Peer retransmits any pending messages with eagle_time > X
        sync_records: Vec<SyncRecord>,
        /// The source address the responder observed this pong's ping arriving from — i.e. the requester's reflexive (public) address on the live UDP data socket. `None` from legacy peers. This is the peer-echoed STUN primitive: a node learns its own public address from any node whose pong it receives (see the traversal plan, P0), on the exact socket the data flows over — unlike fgtw.org's `cf-connecting-ip`, which only sees the TLS flow and is thus cone-NAT-only.
        observed_addr: Option<SocketAddr>,
    },
    // NOTE: ClutchOffer, ClutchInit, ClutchResponse, ClutchComplete REMOVED Full 8-primitive CLUTCH uses ClutchOffer and ClutchKemResponse which are handled via build_clutch_offer_vsf() and parse_clutch_offer_vsf() See CLUTCH.md Section 4.2 for the slot-based ceremony protocol.
    /// Encrypted chat message
    ///
    /// Format: section "msg" with encrypted payload per BRAID.md §9 (wire format)
    /// - conversation_token: smear_hash(sorted participant identity seeds) - privacy-preserving
    /// - prev_msg_hp: hash chain link to previous message (or first_message_anchor)
    /// - ciphertext: encrypted [x(text), hM(confirm_smear)] section
    ChatMessage {
        timestamp: i64,
        /// Privacy-preserving conversation token (smear_hash of sorted participant seeds)
        conversation_token: [u8; 32],
        prev_msg_hp: [u8; 32],
        ciphertext: Vec<u8>,
        sender_pubkey: DevicePubkey,
        signature: [u8; 64],
    },
    /// Message acknowledgment
    ///
    /// Confirms receipt of a message by eagle_time (no sequence numbers). Per BRAID.md §9.2 (ACK):
    /// - acked_eagle_time: which message we're ACKing (i64 oscillations from their header)
    /// - plaintext_hash: proves we decrypted correctly (BLAKE3 of decrypted content)
    MessageAck {
        timestamp: i64,
        /// Privacy-preserving conversation token (smear_hash of sorted participant seeds)
        conversation_token: [u8; 32],
        /// Eagle time oscillations of the message being ACKed (from their VSF header)
        acked_eagle_time: i64,
        /// BLAKE3 hash of decrypted plaintext - proves we decrypted correctly
        plaintext_hash: [u8; 32],
        sender_pubkey: DevicePubkey,
        signature: [u8; 64],
    },
    /// Phonebook gossip REQUEST — "send me your peer list". The peers-are-FGTW mesh: when fgtw.org is unreachable (or just to stay fresh), ask known peers for their phonebook and merge by Eagle-time. Signed like a ping so the responder can authenticate + route the reply to `sender_pubkey`.
    PhonebookRequest {
        timestamp: i64,
        sender_pubkey: DevicePubkey,
        provenance_hash: [u8; 32], // BLAKE3(sender_pubkey || timestamp)
        signature: [u8; 64],       // Ed25519 over provenance_hash
    },
    /// Phonebook gossip RESPONSE — our known peer records, each SELF-SIGNED (see PeerRecord). The receiver verifies + merge_peers each one, so trust rides the record, not this relay.
    PhonebookResponse {
        timestamp: i64,
        responder_pubkey: DevicePubkey,
        provenance_hash: [u8; 32], // echoes the request's provenance (proves we saw it)
        signature: [u8; 64],
        peers: Vec<PeerRecord>,
    },
    /// Avatar exchange REQUEST — "send me your avatar (directly)". Sent peer-to-peer to a MUTUAL contact (a completed CLUTCH ceremony = both added each other), so a friend's avatar comes from the friend, not a public lookup. The responder authenticates `sender_pubkey` against its own contacts and replies ONLY if that peer is a Complete contact — otherwise it stays silent and the requester falls back to FGTW (or nothing). Signed like a ping.
    AvatarRequest {
        timestamp: i64,
        sender_pubkey: DevicePubkey,
        provenance_hash: [u8; 32], // BLAKE3(sender_pubkey || timestamp)
        signature: [u8; 64],       // Ed25519 over provenance_hash
    },
    /// Avatar exchange RESPONSE — the responder's OWN avatar as raw VSF bytes (the same AVIF-in-VSF blob stored in their vault / published to FGTW). Tens of KB, so PT fragments it transparently. The receiver decodes it the same way as an FGTW download. Signed over the avatar bytes' hash.
    AvatarResponse {
        timestamp: i64,
        responder_pubkey: DevicePubkey,
        provenance_hash: [u8; 32], // BLAKE3(avatar_vsf) — also what the signature covers
        signature: [u8; 64],
        avatar_vsf: Vec<u8>,
    },
    /// Address-reflection REQUEST (open-tier STUN) — "tell me the source address you see me at". Body-less + signed like a ping, but answerable by ANY node serving the directory (not just contacts) — this is the peer-echoed reflexive primitive that lets a node learn its own public address on the live UDP data socket without a central STUN server. Reveals only the requester's own address; the response is the same size (no amplification).
    Reflect {
        timestamp: i64,
        sender_pubkey: DevicePubkey,
        provenance_hash: [u8; 32], // BLAKE3(sender_pubkey || timestamp)
        signature: [u8; 64],       // Ed25519 over provenance_hash
    },
    /// Address-reflection RESPONSE — echoes the source address the responder observed the [`FgtwMessage::Reflect`] arrive from, i.e. the requester's reflexive address.
    ReflectResponse {
        timestamp: i64,
        responder_pubkey: DevicePubkey,
        provenance_hash: [u8; 32], // echoes the request's provenance (proves we saw it)
        signature: [u8; 64],
        observed_addr: SocketAddr,
    },
    /// Hole-punch PROBE (friend tier) — a signed datagram fired at a peer's candidate address to open our NAT toward them and elicit an ack. Contact/fleet-gated like ping (only a friend's punch is answered). Body-less; the header `provenance_hash` is the fresh per-probe correlator the ack echoes back.
    PunchProbe {
        timestamp: i64,
        sender_pubkey: DevicePubkey,
        provenance_hash: [u8; 32], // fresh per-probe; the ack echoes it for correlation
        signature: [u8; 64],
    },
    /// Hole-punch ACK — the reply to a [`FgtwMessage::PunchProbe`]. Echoes the probe's provenance so the prober can match which candidate round-tripped (that `(local, remote)` pair is then validated), and carries `observed_addr` so the ack doubles as a reflexive echo.
    PunchProbeAck {
        timestamp: i64,
        responder_pubkey: DevicePubkey,
        provenance_hash: [u8; 32], // echoes the probe's provenance
        signature: [u8; 64],
        observed_addr: SocketAddr,
    },
}

/// Peer record - one device for a user handle.
///
/// Self-attesting: `signature` is the device's own Ed25519 signature over its other fields, made with the secret half of `device_pubkey` (an Ed25519 verifying key — see [`DevicePubkey`]). This is what lets a record propagate thru phonebook GOSSIP without trusting the relay: anyone can `verify()` it against the embedded `device_pubkey`, so a relaying peer can carry a device's entry but cannot forge or redirect its address without breaking the signature. FGTW-sourced records get the same treatment, so both sources of truth (the bootstrap server and peer gossip) are verifiable the same way. A record whose signature doesn't verify is dropped on merge.
#[derive(Debug, Clone)]
pub struct PeerRecord {
    pub handle_proof: [u8; 32], // Memory-hard PoW output (24MB, 17 rounds)
    pub device_pubkey: DevicePubkey, // Device's Ed25519 identity key (also the gossip signature key)
    pub ip: SocketAddr,         // Where to reach this device (public IP)
    pub local_ip: Option<std::net::IpAddr>, // LAN IP for hairpin NAT (peers behind same public IP)
    pub last_seen: i64,         // Eagle Time oscillations
    pub signature: [u8; 64],    // Ed25519 sig by device_pubkey over signing_bytes(); [0;64] = unsigned
}

/// Sync record for pong - tells peer our last received message timestamp per conversation Used for efficient resync: peer retransmits pending messages with eagle_time > last_received_ef6
#[derive(Debug, Clone)]
pub struct SyncRecord {
    /// Privacy-preserving conversation token (smear_hash of sorted participant seeds)
    pub conversation_token: [u8; 32],
    /// Eagle time oscillations of last message received from peer in this conversation Peer should retransmit any pending messages with eagle_time > this value
    pub last_received_osc: i64,
}

/// Convert SocketAddr to binary format for VSF Format:
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

/// Convert binary format back to SocketAddr Returns None if the format is invalid
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

        let builder = VsfBuilder::new().creation_time_oscillations(vsf::eagle_time_oscillations());

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
                    fields.push((
                        format!("{}_last_seen", prefix),
                        VsfType::e(vsf::types::EtType::e6(peer.last_seen)),
                    ));
                    // Self-signature (ge) — see PeerRecord. Carried so the receiver can verify the record without trusting this relay. extract_peer_list reads it back at {prefix}_sig.
                    fields.push((
                        format!("{}_sig", prefix),
                        VsfType::ge(peer.signature.to_vec()),
                    ));
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
                        VsfType::e(vsf::types::EtType::e6(device.last_seen)),
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
                        VsfType::e(vsf::types::EtType::e6(device.last_seen)),
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
                // Simplified header-only format: RÅ< ... ke[pubkey] ge[sig] n1 (ping) > All crypto is in header, section just identifies message type Avatar is NOT included - fetched by handle instead
                builder
                    .creation_time_oscillations(*timestamp)
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
                observed_addr,
            } => {
                // Pong with sync records for efficient resync Format: RÅ< ... ke[pubkey] ge[sig] > [pong (sync_count: N) (sync_0_tok: hb) (sync_0_ef6: f6) (obs: hb)?]
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
                        format!("sync_{}_osc", i),
                        VsfType::e(vsf::types::EtType::e6(record.last_received_osc)),
                    ));
                }
                // Peer-echoed reflexive address: the src we saw the ping come from, so the requester learns its own public address on the data socket. Absent → legacy/unknown, parses back to None.
                if let Some(addr) = observed_addr {
                    fields.push(("obs".to_string(), VsfType::hb(socketaddr_to_bytes(addr))));
                }
                builder
                    .creation_time_oscillations(*timestamp)
                    .provenance_hash(*provenance_hash)
                    .signature_ed25519(*responder_pubkey.as_bytes(), *signature)
                    .add_section("pong", fields)
                    .build()
            }
            // NOTE: ClutchOffer, ClutchInit, ClutchResponse, ClutchComplete serialization REMOVED Full CLUTCH uses build_clutch_offer_vsf() and build_clutch_kem_response_vsf()
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
                    .creation_time_oscillations(*timestamp)
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
                    .creation_time_oscillations(*timestamp)
                    .provenance_hash(provenance)
                    .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                    .add_section(
                        "ack",
                        vec![
                            ("tok".to_string(), VsfType::hg(conversation_token.to_vec())),
                            (
                                "time".to_string(),
                                VsfType::e(vsf::types::EtType::e6(*acked_eagle_time)),
                            ),
                            ("hash".to_string(), VsfType::hb(plaintext_hash.to_vec())),
                        ],
                    )
                    .build()
            }
            FgtwMessage::PhonebookRequest {
                timestamp,
                sender_pubkey,
                provenance_hash,
                signature,
            } => builder
                .creation_time_oscillations(*timestamp)
                .provenance_hash(*provenance_hash)
                .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                .add_section("pb_req", vec![])
                .build(),
            FgtwMessage::PhonebookResponse {
                timestamp,
                responder_pubkey,
                provenance_hash,
                signature,
                peers,
            } => {
                // One multi-value `peer` field per record (positional, parse_peer_from_field shape). add_section only takes single-value fields, so build the section directly.
                let mut section = vsf::VsfSection::new("pb_resp");
                for peer in peers {
                    let (name, values) = encode_peer_field(peer);
                    section.add_field_multi(name, values);
                }
                builder
                    .creation_time_oscillations(*timestamp)
                    .provenance_hash(*provenance_hash)
                    .signature_ed25519(*responder_pubkey.as_bytes(), *signature)
                    .add_section_direct(section)
                    .build()
            }
            FgtwMessage::Reflect {
                timestamp,
                sender_pubkey,
                provenance_hash,
                signature,
            } => builder
                .creation_time_oscillations(*timestamp)
                .provenance_hash(*provenance_hash)
                .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                .add_section("reflect", vec![])
                .build(),
            FgtwMessage::ReflectResponse {
                timestamp,
                responder_pubkey,
                provenance_hash,
                signature,
                observed_addr,
            } => builder
                .creation_time_oscillations(*timestamp)
                .provenance_hash(*provenance_hash)
                .signature_ed25519(*responder_pubkey.as_bytes(), *signature)
                .add_section(
                    "reflect_resp",
                    vec![(
                        "obs".to_string(),
                        VsfType::hb(socketaddr_to_bytes(observed_addr)),
                    )],
                )
                .build(),
            FgtwMessage::PunchProbe {
                timestamp,
                sender_pubkey,
                provenance_hash,
                signature,
            } => builder
                .creation_time_oscillations(*timestamp)
                .provenance_hash(*provenance_hash)
                .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                .add_section("punch", vec![])
                .build(),
            FgtwMessage::PunchProbeAck {
                timestamp,
                responder_pubkey,
                provenance_hash,
                signature,
                observed_addr,
            } => builder
                .creation_time_oscillations(*timestamp)
                .provenance_hash(*provenance_hash)
                .signature_ed25519(*responder_pubkey.as_bytes(), *signature)
                .add_section(
                    "punch_ack",
                    vec![(
                        "obs".to_string(),
                        VsfType::hb(socketaddr_to_bytes(observed_addr)),
                    )],
                )
                .build(),
            FgtwMessage::AvatarRequest {
                timestamp,
                sender_pubkey,
                provenance_hash,
                signature,
            } => builder
                .creation_time_oscillations(*timestamp)
                .provenance_hash(*provenance_hash)
                .signature_ed25519(*sender_pubkey.as_bytes(), *signature)
                .add_section("av_req", vec![])
                .build(),
            FgtwMessage::AvatarResponse {
                timestamp,
                responder_pubkey,
                provenance_hash,
                signature,
                avatar_vsf,
            } => builder
                .creation_time_oscillations(*timestamp)
                .provenance_hash(*provenance_hash)
                .signature_ed25519(*responder_pubkey.as_bytes(), *signature)
                .add_section(
                    "av_resp",
                    vec![(
                        "data".to_string(),
                        VsfType::t_u3(vsf::Tensor::new(vec![avatar_vsf.len()], avatar_vsf.clone())),
                    )],
                )
                .build(),
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

        let (header, header_end) =
            VsfHeader::decode(bytes).map_err(|e| format!("Failed to parse VSF header: {}", e))?;

        // Check for empty section (header-only format like ping/pong) Empty sections have no '[' after '>' - the section name is in the header field
        if header_end >= bytes.len() || bytes[header_end] != b'[' {
            // No section body - search header fields for message type (ping/pong)
            let section_name = header
                .fields
                .iter()
                .find(|f| {
                    f.name == "ping"
                        || f.name == "pong"
                        || f.name == "pb_req"
                        || f.name == "av_req"
                        || f.name == "reflect"
                        || f.name == "punch"
                })
                .map(|f| f.name.as_str());

            if let Some(section_name) = section_name {
                let timestamp = extract_header_timestamp(&header)?;
                let pubkey = extract_header_pubkey(&header)?;
                let provenance_hash = extract_header_provenance(&header)?;
                let signature = extract_header_signature(&header)?;

                if section_name == "pb_req" {
                    // Phonebook request is body-less (empty section), so it lands on this header-only fast path like ping.
                    return Ok(FgtwMessage::PhonebookRequest {
                        timestamp,
                        sender_pubkey: pubkey,
                        provenance_hash,
                        signature,
                    });
                } else if section_name == "av_req" {
                    // Avatar request is body-less too — same header-only fast path.
                    return Ok(FgtwMessage::AvatarRequest {
                        timestamp,
                        sender_pubkey: pubkey,
                        provenance_hash,
                        signature,
                    });
                } else if section_name == "reflect" {
                    // Address-reflection request is body-less too — header-only fast path like ping.
                    return Ok(FgtwMessage::Reflect {
                        timestamp,
                        sender_pubkey: pubkey,
                        provenance_hash,
                        signature,
                    });
                } else if section_name == "punch" {
                    // Hole-punch probe is body-less — header-only fast path like ping.
                    return Ok(FgtwMessage::PunchProbe {
                        timestamp,
                        sender_pubkey: pubkey,
                        provenance_hash,
                        signature,
                    });
                } else if section_name == "ping" {
                    return Ok(FgtwMessage::StatusPing {
                        timestamp,
                        sender_pubkey: pubkey,
                        provenance_hash,
                        signature,
                    });
                } else {
                    // Old header-only pong format - no sync records, no observed_addr (backwards compat)
                    return Ok(FgtwMessage::StatusPong {
                        timestamp,
                        responder_pubkey: pubkey,
                        provenance_hash,
                        signature,
                        sync_records: vec![],
                        observed_addr: None,
                    });
                }
            }
            return Err("No section found".to_string());
        }

        // Parse section using VsfSection::parse()
        let (section, section_name) = parse_section_after_header(bytes, &header, header_end)
            .map_err(|e| format!("Failed to parse section: {}", e))?;

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
                // Pong - parse sync records + optional observed_addr from section body
                let fields = section_fields_to_tuples(&section);
                let sync_records = extract_sync_records(&fields)?;
                let observed_addr = extract_observed_addr(&fields);

                return Ok(FgtwMessage::StatusPong {
                    timestamp,
                    responder_pubkey: pubkey,
                    provenance_hash,
                    signature,
                    sync_records,
                    observed_addr,
                });
            }
        }

        // Address-reflection response: header carries the signed provenance, the `obs` field the observed address.
        if section_name == "reflect_resp" {
            let timestamp = extract_header_timestamp(&header)?;
            let pubkey = extract_header_pubkey(&header)?;
            let provenance_hash = extract_header_provenance(&header)?;
            let signature = extract_header_signature(&header)?;
            let fields = section_fields_to_tuples(&section);
            let observed_addr = extract_observed_addr(&fields)
                .ok_or_else(|| "reflect_resp missing observed_addr".to_string())?;
            return Ok(FgtwMessage::ReflectResponse {
                timestamp,
                responder_pubkey: pubkey,
                provenance_hash,
                signature,
                observed_addr,
            });
        }

        // Hole-punch ack: same shape as reflect_resp (echoed provenance in the header, observed address in `obs`).
        if section_name == "punch_ack" {
            let timestamp = extract_header_timestamp(&header)?;
            let pubkey = extract_header_pubkey(&header)?;
            let provenance_hash = extract_header_provenance(&header)?;
            let signature = extract_header_signature(&header)?;
            let fields = section_fields_to_tuples(&section);
            let observed_addr = extract_observed_addr(&fields)
                .ok_or_else(|| "punch_ack missing observed_addr".to_string())?;
            return Ok(FgtwMessage::PunchProbeAck {
                timestamp,
                responder_pubkey: pubkey,
                provenance_hash,
                signature,
                observed_addr,
            });
        }

        // NOTE: clutch_offer, clutch_init, clutch_resp, clutch_done deserialization REMOVED Full CLUTCH uses parse_clutch_offer_vsf() and parse_clutch_kem_response_vsf() which handle "clutch_offer" and "clutch_kem_response" sections

        // Handle msg (encrypted chat message) and ack (acknowledgment)
        if section_name == "msg" || section_name == "ack" {
            let timestamp = extract_header_timestamp(&header)?;
            let sender_pubkey = extract_header_pubkey(&header)?;
            let signature = extract_header_signature(&header)?;

            let fields = section_fields_to_tuples(&section);

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
                // MessageAck: tok (conversation_token), time (acked_eagle_time), hash (plaintext_hash) No sequence numbers, no weave (deferred)
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

        // Phonebook gossip request/response
        if section_name == "pb_req" || section_name == "pb_resp" {
            let timestamp = extract_header_timestamp(&header)?;
            let pubkey = extract_header_pubkey(&header)?;
            let provenance_hash = extract_header_provenance(&header)?;
            let signature = extract_header_signature(&header)?;

            if section_name == "pb_req" {
                return Ok(FgtwMessage::PhonebookRequest {
                    timestamp,
                    sender_pubkey: pubkey,
                    provenance_hash,
                    signature,
                });
            } else {
                // Each `peer` field is a positional multi-value record; decode with the production parser (the same one the FGTW peer list uses). A record that fails to decode is skipped rather than failing the whole response.
                let peers: Vec<PeerRecord> = section
                    .get_fields("peer")
                    .iter()
                    .filter_map(|f| {
                        crate::network::fgtw::bootstrap::parse_peer_from_field(f).ok()
                    })
                    .collect();
                return Ok(FgtwMessage::PhonebookResponse {
                    timestamp,
                    responder_pubkey: pubkey,
                    provenance_hash,
                    signature,
                    peers,
                });
            }
        }

        // Avatar exchange response — carries the responder's own avatar VSF bytes in `data`.
        if section_name == "av_resp" {
            let timestamp = extract_header_timestamp(&header)?;
            let pubkey = extract_header_pubkey(&header)?;
            let provenance_hash = extract_header_provenance(&header)?;
            let signature = extract_header_signature(&header)?;
            let fields = section_fields_to_tuples(&section);
            let avatar_vsf = extract_data(&fields, "data")?;
            return Ok(FgtwMessage::AvatarResponse {
                timestamp,
                responder_pubkey: pubkey,
                provenance_hash,
                signature,
                avatar_vsf,
            });
        }

        // Original fgtw section handling
        if section_name != "fgtw" {
            return Err(format!(
                "Expected 'fgtw', 'ping'/'pong', 'clutch_*', 'msg', 'ack', 'pb_*', or 'av_*' section, got '{}'",
                section_name
            ));
        }

        let fields = section_fields_to_tuples(&section);

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
            last_seen: vsf::eagle_time_oscillations(),
            signature: [0u8; 64],
        }
    }

    /// Canonical bytes the device signs / a verifier checks: handle_proof ‖ device_pubkey ‖ ip ‖ local_ip ‖ last_seen, length-tagged so no field can bleed into the next (an injective framing, the same discipline the braid uses). IP/local_ip serialize via their `to_string()` so a v4 and its v4-mapped-v6 form sign distinctly — which is correct, they're different reachability facts. EXCLUDES `signature` itself. Anything in here that an attacker changes (e.g. the address) invalidates the signature, which is the whole point.
    fn signing_bytes(&self) -> Vec<u8> {
        let ip = self.ip.to_string();
        let local = self.local_ip.map(|a| a.to_string()).unwrap_or_default();
        let mut out = Vec::with_capacity(32 + 32 + ip.len() + local.len() + 8 + 16);
        out.extend_from_slice(b"PHOTON_PEER_RECORD_v0");
        out.extend_from_slice(&self.handle_proof);
        out.extend_from_slice(self.device_pubkey.as_bytes());
        out.extend_from_slice(&(ip.len() as u32).to_le_bytes());
        out.extend_from_slice(ip.as_bytes());
        out.extend_from_slice(&(local.len() as u32).to_le_bytes());
        out.extend_from_slice(local.as_bytes());
        out.extend_from_slice(&self.last_seen.to_le_bytes());
        out
    }

    /// Self-sign this record with the device's Ed25519 secret key. The secret MUST be the half of `device_pubkey`; callers pass their own device keypair (the one derived from the machine fingerprint), so a device only ever signs its OWN entry. Fills `signature` in place.
    pub fn sign(&mut self, signing_key: &ed25519_dalek::SigningKey) {
        use ed25519_dalek::Signer;
        let sig = signing_key.sign(&self.signing_bytes());
        self.signature = sig.to_bytes();
    }

    /// Verify the self-signature against the embedded `device_pubkey`. `true` only if the signature was made by the holder of that device's secret over THESE exact fields — so a gossiped record can be trusted without trusting the relay that carried it. An all-zero signature (legacy / unsigned FGTW record) verifies `false`.
    pub fn verify(&self) -> bool {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        let Ok(vk) = VerifyingKey::from_bytes(self.device_pubkey.as_bytes()) else {
            return false;
        };
        let sig = Signature::from_bytes(&self.signature);
        vk.verify(&self.signing_bytes(), &sig).is_ok()
    }
}

/// Encode one PeerRecord as a single multi-value `peer` field, in the exact POSITIONAL shape [`crate::network::fgtw::bootstrap::parse_peer_from_field`] reads — the production-proven encoding (FGTW peer lists decode thru it daily): `(peer: hP{handle_proof}, ke{device_pubkey}, t_u3{ip}, u4{port}, e6{last_seen}, t_u3{local_ip}, ge{sig})` The trailing `ge` self-signature lets the receiver verify each record independently of the relay. (The flat-named `peer_N_*` / `v_u3` style of the legacy DHT `extract_peer_list` is deliberately NOT used — it has a latent IP type mismatch and isn't exercised in production.)
fn encode_peer_field(peer: &PeerRecord) -> (String, Vec<VsfType>) {
    let (ip_octets, port) = match peer.ip {
        SocketAddr::V4(v4) => (v4.ip().octets().to_vec(), v4.port()),
        SocketAddr::V6(v6) => (v6.ip().octets().to_vec(), v6.port()),
    };
    // local_ip at index 5 (empty tensor when absent → parses back to None); sig at index 6.
    let local_octets: Vec<u8> = match peer.local_ip {
        Some(IpAddr::V4(v4)) => v4.octets().to_vec(),
        Some(IpAddr::V6(v6)) => v6.octets().to_vec(),
        None => Vec::new(),
    };
    let values = vec![
        VsfType::hP(peer.handle_proof.to_vec()),
        peer.device_pubkey.to_vsf(),
        VsfType::t_u3(vsf::Tensor::new(vec![ip_octets.len()], ip_octets)),
        VsfType::u4(port), // u4 = u16 — a port needs 16 bits (u3 is u8 and would truncate)
        VsfType::e(vsf::types::EtType::e6(peer.last_seen)),
        VsfType::t_u3(vsf::Tensor::new(vec![local_octets.len()], local_octets)),
        VsfType::ge(peer.signature.to_vec()),
    ];
    ("peer".to_string(), values)
}

/// Convert VsfSection fields to (name, value) tuples for helper functions. Fields with no values are skipped; multi-value fields use only the first value.
fn section_fields_to_tuples(section: &vsf::VsfSection) -> Vec<(String, VsfType)> {
    section
        .fields
        .iter()
        .filter_map(|f| f.values.first().map(|v| (f.name.clone(), v.clone())))
        .collect()
}

/// Parse a VsfSection from VSF bytes after a header, resolving the section name from the header TOC when the body has no embedded name (small sections).
fn parse_section_after_header(
    data: &[u8],
    header: &vsf::VsfHeader,
    header_end: usize,
) -> Result<(vsf::VsfSection, String), String> {
    let mut ptr = header_end;
    let mut section = vsf::VsfSection::parse(data, &mut ptr)
        .map_err(|e| format!("Failed to parse section: {}", e))?;
    let name = if section.name.is_empty() {
        header
            .fields
            .first()
            .map(|f| f.name.clone())
            .unwrap_or_default()
    } else {
        section.name.clone()
    };
    section.name = name.clone();
    Ok((section, name))
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

/// Extract Eagle time as i64 oscillations from VSF e() type
fn extract_eagle_time(fields: &[(String, VsfType)], key: &str) -> Result<i64, String> {
    use vsf::types::EtType;
    match get_field(fields, key) {
        Some(VsfType::e(EtType::e6(v))) => Ok(*v),
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
            Some(VsfType::e(vsf::types::EtType::e6(osc))) => *osc,
            _ => return Err(format!("Missing or invalid {}", last_seen_key)),
        };

        // local_ip (v_u3): 4 bytes = v4, 16 = v6, empty = None. Must be decoded (not hardcoded None) so the signature — which covers local_ip — verifies.
        let local_ip = match get_field(fields, &format!("{}_local_ip", peer_prefix)) {
            Some(VsfType::v_u3(v)) if v.data.len() == 4 => Some(IpAddr::V4(std::net::Ipv4Addr::new(
                v.data[0], v.data[1], v.data[2], v.data[3],
            ))),
            Some(VsfType::v_u3(v)) if v.data.len() == 16 => {
                let mut o = [0u8; 16];
                o.copy_from_slice(&v.data);
                Some(IpAddr::V6(std::net::Ipv6Addr::from(o)))
            }
            _ => None,
        };

        let sig_key = format!("{}_sig", peer_prefix);
        let signature = match get_field(fields, &sig_key) {
            Some(VsfType::ge(s)) if s.len() == 64 => s.as_slice().try_into().unwrap(),
            // Legacy / unsigned record (this DHT FoundNodes path predates self-signed records). Left as [0;64]; merge_peer's verify() will reject it, which is the safe default — only signed records propagate thru gossip.
            _ => [0u8; 64],
        };
        peers.push(PeerRecord {
            handle_proof,
            device_pubkey,
            ip,
            local_ip,
            last_seen,
            signature,
        });
    }

    Ok(peers)
}

/// Extract sync records from pong message fields Format: sync_count, sync_0_tok, sync_0_ef6, sync_1_tok, sync_1_ef6, ...
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
        let osc_key = format!("sync_{}_osc", i);

        let conversation_token = extract_hash(fields, &tok_key)?;
        let last_received_osc = match get_field(fields, &osc_key) {
            Some(VsfType::e(vsf::types::EtType::e6(v))) => *v,
            _ => return Err(format!("Missing or invalid {}", osc_key)),
        };

        records.push(SyncRecord {
            conversation_token,
            last_received_osc,
        });
    }

    Ok(records)
}

/// Extract the peer-echoed reflexive address from a pong body, if present. Carried as an `obs` byte field holding [`socketaddr_to_bytes`] (6 bytes v4 / 18 bytes v6). Absent (legacy peers) or malformed → `None`, so this never fails a pong parse.
fn extract_observed_addr(fields: &[(String, VsfType)]) -> Option<SocketAddr> {
    match get_field(fields, "obs") {
        Some(VsfType::hb(bytes)) => bytes_to_socketaddr(bytes),
        _ => None,
    }
}

// Helper functions to extract from VsfHeader for simplified ping/pong format

fn extract_header_timestamp(header: &vsf::file_format::VsfHeader) -> Result<i64, String> {
    use vsf::types::EtType;
    match &header.creation_time {
        Some(VsfType::e(EtType::e6(v))) => Ok(*v),
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
    // Signature is in header.signature field (replaces rolling_hash when present) For now, check if there's a ge signature in the header The VsfHeader struct stores signature in a specific field
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

// Note: extract_header_avatar_id removed - avatar is now fetched by handle Storage key = BLAKE3(BLAKE3(handle) || "avatar")

// NOTE: compute_clutch_provenance and compute_clutch_complete_provenance REMOVED They were only used by the legacy ClutchOffer/ClutchInit/ClutchResponse/ClutchComplete Full CLUTCH uses ceremony_id as provenance (deterministic from handle_hashes)

/// Compute provenance hash for encrypted chat message (CHAIN format) provenance = BLAKE3(conversation_token || prev_msg_hp)
fn compute_chat_provenance(conversation_token: &[u8; 32], prev_msg_hp: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(conversation_token);
    hasher.update(prev_msg_hp);
    *hasher.finalize().as_bytes()
}

/// Compute provenance hash for message acknowledgment (CHAIN format) provenance = BLAKE3(conversation_token || acked_eagle_time_bytes || plaintext_hash || "ack")
fn compute_ack_provenance_v2(
    conversation_token: &[u8; 32],
    acked_eagle_time: i64,
    plaintext_hash: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(conversation_token);
    hasher.update(&acked_eagle_time.to_le_bytes());
    hasher.update(plaintext_hash);
    hasher.update(b"ack");
    *hasher.finalize().as_bytes()
}

// ============================================================================= VSF-WRAPPED CLUTCH MESSAGES (Full 8-Algorithm CLUTCH) =============================================================================

use crate::crypto::clutch::{ClutchCompletePayload, ClutchKemResponsePayload, ClutchOfferPayload};

// NOTE: ceremony_id is now computed deterministically via CeremonyId::derive() from sorted participant handle_hashes. No memory-hard hashing needed. See src/types/friendship.rs for the implementation.

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
/// The ceremony_id is deterministic from CeremonyId::derive(&[handle_hashes]). Both parties compute the same value independently - no echo needed.
///
/// Returns signed VSF bytes ready for transmission.
///
/// conversation_token: Privacy-preserving smear_hash of sorted participant identity seeds. Replaces handle_hashes to prevent identity correlation by network observers.
///
/// Note: The offer_provenance is computed deterministically from the public keys. This ensures both parties compute identical provenances regardless of timestamp. The ceremony_id is computed later from all parties' offer provenances via spaghettify. Returns (vsf_bytes, offer_provenance) - the signed VSF and the key-based provenance.
pub fn build_clutch_offer_vsf(
    conversation_token: &[u8; 32],
    payload: &ClutchOfferPayload,
    device_pubkey: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<(Vec<u8>, [u8; 32]), String> {
    use vsf::VsfBuilder;

    // Build unsigned VSF with signature placeholder hp (provenance hash) will be auto-computed by sign_file from the content This hash is unique per offer due to timestamp and content
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
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signature_ed25519(*device_pubkey, [0u8; 64])
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("Failed to build ClutchOffer VSF: {}", e))?;

    // Sign the file (computes file hash, signs it, patches ge)
    let signed = vsf::verification::sign_file(unsigned, device_secret)?;

    // Compute offer_provenance from keys (deterministic, no timestamp) Hash all pubkeys in fixed order: x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc
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
    // Verified read: is_original (content hp — the offer is built canonically by sign_file) + Ed25519 signature, un-skippable and in the right order. Replaces the separate verify_file_signature + bare decode pair.
    let (header, header_end) = vsf::verification::read_verified(vsf_bytes, None)
        .map_err(|e| format!("ClutchOffer verification failed: {}", e))?;

    // Extract signer pubkey
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    // offer_provenance will be computed from keys after parsing (deterministic, no timestamp)

    let (section, section_name) = parse_section_after_header(vsf_bytes, &header, header_end)?;

    if section_name != "clutch_offer" {
        return Err(format!(
            "Expected 'clutch_offer' section, got '{}'",
            section_name
        ));
    }

    let fields = &section.fields;

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

    // Extract pubkeys from multi-value "pubkeys" field (kx, kp, kk, kp, kf, kn, kl, kh order) Also support legacy individual fields for backwards compatibility
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

    // Compute offer_provenance from keys (deterministic, no timestamp) Hash all pubkeys in fixed order: x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc
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
            // `.min(8)` guards a short (forged / truncated) field — a bare `[..8]` panics the whole receiver task.
            hex::encode(&payload.hqc256_public[..payload.hqc256_public.len().min(8)])
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
        .creation_time_oscillations(vsf::eagle_time_oscillations())
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
/// Returns (payload, sender_pubkey, ceremony_id, conversation_token) conversation_token is the privacy-preserving smear_hash of sorted participant identity seeds.
pub fn parse_clutch_kem_response_vsf(
    vsf_bytes: &[u8],
    expected_conversation_token: &[u8; 32],
) -> Result<(ClutchKemResponsePayload, [u8; 32], [u8; 32], [u8; 32]), String> {
    // SPEC DEVIATION (messaging rework will resolve): this frame's hp carries the ceremony_id — application semantics, not content provenance — so is_original/read_verified would reject it by design. The scheme-1 signature below still covers the ENTIRE file (including that hp), so integrity + authorship ARE verified; only content-hp self-attestation is waived. Canonicalising means moving ceremony_id into a section field (wire change). Verify signature first
    if !vsf::verification::verify_file_signature(vsf_bytes)? {
        return Err("Invalid signature on ClutchKemResponse".to_string());
    }

    // Extract signer pubkey
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    // Parse header and section
    use vsf::file_format::VsfHeader;
    let (header, header_end) =
        VsfHeader::decode(vsf_bytes).map_err(|e| format!("Failed to parse header: {}", e))?;

    let ceremony_id = extract_header_provenance(&header)?;

    let (section, section_name) = parse_section_after_header(vsf_bytes, &header, header_end)?;

    if section_name != "clutch_kem_response" {
        return Err(format!(
            "Expected 'clutch_kem_response' section, got '{}'",
            section_name
        ));
    }

    let fields = &section.fields;

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

    // Extract ciphertexts from multi-value "ciphertexts" field (vf, vn, vl, vc order) Also support legacy individual fields for backwards compatibility
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

    // Extract EC ephemeral pubkeys from multi-value "ephemerals" field (kx, kp, kk, kp order) Also support legacy individual fields for backwards compatibility
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
            // `.min(8)` guards a short field so a truncated/forged ciphertext can't panic the receiver.
            hex::encode(&payload.hqc256_ciphertext[..payload.hqc256_ciphertext.len().min(8)]),
            payload.x25519_ephemeral.len(),
            payload.p384_ephemeral.len()
        ));
    }

    Ok((payload, sender_pubkey, ceremony_id, conversation_token))
}

/// Parse and verify a VSF ClutchOffer message WITHOUT recipient check.
///
/// This variant is used by the TCP receiver which doesn't know our conversation_token. The caller (app.rs) is responsible for verifying the message is addressed to them.
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
    // Verified read: is_original (content hp — the offer is built canonically by sign_file) + Ed25519 signature, un-skippable and in the right order. Replaces the separate verify_file_signature + bare decode pair.
    let (header, header_end) = vsf::verification::read_verified(vsf_bytes, None)
        .map_err(|e| format!("ClutchOffer verification failed: {}", e))?;

    // Extract signer pubkey
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    // offer_provenance will be computed from keys after parsing (deterministic, no timestamp)

    let (section, section_name) = parse_section_after_header(vsf_bytes, &header, header_end)?;

    if section_name != "clutch_offer" {
        return Err(format!(
            "Expected 'clutch_offer' section, got '{}'",
            section_name
        ));
    }

    let fields = &section.fields;

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

    // Extract pubkeys from multi-value "pubkeys" field (kx, kp, kk, kp, kf, kn, kl, kh order) Also support legacy individual fields for backwards compatibility
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

    // Compute offer_provenance from keys (deterministic, no timestamp) Hash all pubkeys in fixed order: x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc
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
        // `.min(8)` guards a short field so a truncated/forged public key can't panic the receiver (offer_provenance is a fixed [u8;32], so its slice is always in-bounds).
        hex::encode(&payload.hqc256_public[..payload.hqc256_public.len().min(8)]),
        hex::encode(&offer_provenance[..8])
    ));

    Ok((payload, sender_pubkey, offer_provenance, conversation_token))
}

/// Parse and verify a VSF ClutchKemResponse message WITHOUT recipient check.
///
/// This variant is used by the TCP receiver which doesn't know our conversation_token. The caller (app.rs) is responsible for verifying the message is addressed to them.
///
/// Verifies:
/// 1. VSF format and magic bytes
/// 2. Ed25519 signature (header-level)
///
/// Returns (payload, sender_pubkey, ceremony_id, conversation_token)
pub fn parse_clutch_kem_response_vsf_without_recipient_check(
    vsf_bytes: &[u8],
) -> Result<(ClutchKemResponsePayload, [u8; 32], [u8; 32], [u8; 32]), String> {
    // SPEC DEVIATION (messaging rework will resolve): this frame's hp carries the ceremony_id — application semantics, not content provenance — so is_original/read_verified would reject it by design. The scheme-1 signature below still covers the ENTIRE file (including that hp), so integrity + authorship ARE verified; only content-hp self-attestation is waived. Canonicalising means moving ceremony_id into a section field (wire change). Verify signature first
    if !vsf::verification::verify_file_signature(vsf_bytes)? {
        return Err("Invalid signature on ClutchKemResponse".to_string());
    }

    // Extract signer pubkey
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    // Parse header and section
    use vsf::file_format::VsfHeader;
    let (header, header_end) =
        VsfHeader::decode(vsf_bytes).map_err(|e| format!("Failed to parse header: {}", e))?;

    let ceremony_id = extract_header_provenance(&header)?;

    let (section, section_name) = parse_section_after_header(vsf_bytes, &header, header_end)?;

    if section_name != "clutch_kem_response" {
        return Err(format!(
            "Expected 'clutch_kem_response' section, got '{}'",
            section_name
        ));
    }

    let fields = &section.fields;

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

    // Extract ciphertexts from multi-value "ciphertexts" field (vf, vn, vl, vc order) Also support legacy individual fields for backwards compatibility
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

    // Extract EC ephemeral pubkeys from multi-value "ephemerals" field (kx, kp, kk, kp order) Also support legacy individual fields for backwards compatibility
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
        // `.min(8)` guards a short field so a truncated/forged ciphertext can't panic the receiver.
        hex::encode(&payload.hqc256_ciphertext[..payload.hqc256_ciphertext.len().min(8)]),
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

// ============================================================================= CLUTCH COMPLETE (Proof Exchange) =============================================================================

/// Build a signed VSF ClutchComplete message (~200 bytes).
///
/// Contains the eggs_proof hash for verification. Both parties send this after computing their eggs, and verify the peer's proof matches.
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
        .creation_time_oscillations(vsf::eagle_time_oscillations())
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
    // SPEC DEVIATION (messaging rework will resolve): this frame's hp carries the ceremony_id — application semantics, not content provenance — so is_original/read_verified would reject it by design. The scheme-1 signature below still covers the ENTIRE file (including that hp), so integrity + authorship ARE verified; only content-hp self-attestation is waived. Canonicalising means moving ceremony_id into a section field (wire change). Verify signature first
    if !vsf::verification::verify_file_signature(vsf_bytes)? {
        return Err("Invalid signature on ClutchComplete".to_string());
    }

    // Extract signer pubkey
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    // Parse header and section
    use vsf::file_format::VsfHeader;
    let (header, header_end) =
        VsfHeader::decode(vsf_bytes).map_err(|e| format!("Failed to parse header: {}", e))?;

    let ceremony_id = extract_header_provenance(&header)?;

    let (section, section_name) = parse_section_after_header(vsf_bytes, &header, header_end)?;

    if section_name != "clutch_complete" {
        return Err(format!(
            "Expected 'clutch_complete' section, got '{}'",
            section_name
        ));
    }

    let fields = section_fields_to_tuples(&section);

    // Extract conversation_token
    let conversation_token = extract_spaghetti_hash(&fields, "tok")?;

    // Verify conversation_token matches expected
    if &conversation_token != expected_conversation_token {
        return Err("ClutchComplete conversation_token mismatch".to_string());
    }

    // Extract eggs_proof
    let eggs_proof = extract_spaghetti_hash(&fields, "eggs_proof")?;

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
/// This variant is used by the TCP receiver which doesn't know our conversation_token. The caller (app.rs) is responsible for verifying the message is addressed to them.
pub fn parse_clutch_complete_vsf_without_recipient_check(
    vsf_bytes: &[u8],
) -> Result<(ClutchCompletePayload, [u8; 32], [u8; 32], [u8; 32]), String> {
    // SPEC DEVIATION (messaging rework will resolve): this frame's hp carries the ceremony_id — application semantics, not content provenance — so is_original/read_verified would reject it by design. The scheme-1 signature below still covers the ENTIRE file (including that hp), so integrity + authorship ARE verified; only content-hp self-attestation is waived. Canonicalising means moving ceremony_id into a section field (wire change). Verify signature first
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

    let (section, section_name) = parse_section_after_header(vsf_bytes, &header, header_end)?;

    if section_name != "clutch_complete" {
        return Err(format!(
            "Expected 'clutch_complete' section, got '{}'",
            section_name
        ));
    }

    let fields = section_fields_to_tuples(&section);

    // Extract conversation_token (NO recipient check - caller verifies)
    let conversation_token = extract_spaghetti_hash(&fields, "tok")?;

    // Extract eggs_proof
    let eggs_proof = extract_spaghetti_hash(&fields, "eggs_proof")?;

    let payload = ClutchCompletePayload { eggs_proof };

    #[cfg(feature = "development")]
    crate::log(&format!(
        "CLUTCH: Parsed complete proof (no recipient check) proof={}...",
        hex::encode(&payload.eggs_proof[..8])
    ));

    Ok((payload, sender_pubkey, ceremony_id, conversation_token))
}

// ============================================================================ History recovery frames — hist_req / hist_page ============================================================================
//
// Standalone signed VSF frames for friend-assisted conversation backfill (newest-first cursor pagination). Both are built CANONICALLY (sign_file computes the content-hp provenance) and parsed via read_verified + parse_section_after_header — no raw decode sites, vsf-gate untouched. The page payload is an opaque AEAD blob sealed under the friendship history key (see network::history_pages); the wire leaks only the conversation token + blob size.

/// A parsed history request: "send me up to `limit` rows strictly OLDER than `before_osc`".
#[derive(Clone, Debug)]
pub struct HistoryRequestPayload {
    pub conversation_token: [u8; 32],
    /// Cursor: rows strictly older than this eagle-time. `i64::MAX` = head page.
    pub before_osc: i64,
    /// Requested row cap (server clamps to its own page maximum).
    pub limit: u32,
    /// Random request id, echoed in the page — correlates request↔response and dedups replays.
    pub request_id: [u8; 32],
    /// Header creation time (eagle oscillations) — the responder's staleness check.
    pub sent_osc: i64,
}

/// Build a signed `hist_req` frame (~200 bytes).
pub fn build_history_request_vsf(
    conversation_token: &[u8; 32],
    before_osc: i64,
    limit: u32,
    request_id: &[u8; 32],
    device_pubkey: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<Vec<u8>, String> {
    use vsf::file_format::VsfSection;
    use vsf::VsfBuilder;

    let mut section = VsfSection::new("hist_req");
    section.add_field("tok", VsfType::hg(conversation_token.to_vec()));
    section.add_field("before", VsfType::e(vsf::types::EtType::e6(before_osc)));
    section.add_field("limit", VsfType::u5(limit));
    section.add_field("rid", VsfType::hb(request_id.to_vec()));

    let unsigned = VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signature_ed25519(*device_pubkey, [0u8; 64])
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("Failed to build hist_req VSF: {}", e))?;

    // Canonical scheme-1: sign_file computes the content hp + ge over the whole file.
    vsf::verification::sign_file(unsigned, device_secret)
}

/// Parse + verify a `hist_req` frame. Returns (payload, sender_pubkey). The caller authorizes the sender against the conversation's contact (known device + mutual) — signature validity alone is NOT authorization.
pub fn parse_history_request_vsf(
    vsf_bytes: &[u8],
) -> Result<(HistoryRequestPayload, [u8; 32]), String> {
    // Verified read: is_original (content hp) + Ed25519 signature, un-skippable and in order.
    let (header, header_end) = vsf::verification::read_verified(vsf_bytes, None)
        .map_err(|e| format!("hist_req verification failed: {}", e))?;
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    let sent_osc = header_creation_oscillations(&header);

    let (section, section_name) = parse_section_after_header(vsf_bytes, &header, header_end)?;
    if section_name != "hist_req" {
        return Err(format!("Expected 'hist_req' section, got '{}'", section_name));
    }
    let fields = &section.fields;

    let conversation_token = field_hash32(fields, "tok", |v| matches!(v, VsfType::hg(_)))
        .ok_or("hist_req missing tok")?;
    let before_osc = fields
        .iter()
        .find(|f| f.name == "before")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::e(vsf::types::EtType::e6(osc)) => Some(*osc),
            _ => None,
        })
        .ok_or("hist_req missing before")?;
    let limit = fields
        .iter()
        .find(|f| f.name == "limit")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::u3(n) => Some(*n as u32),
            VsfType::u4(n) => Some(*n as u32),
            VsfType::u5(n) => Some(*n),
            VsfType::u6(n) => Some(*n as u32),
            _ => None,
        })
        .ok_or("hist_req missing limit")?;
    let request_id =
        field_hash32(fields, "rid", |v| matches!(v, VsfType::hb(_))).ok_or("hist_req missing rid")?;

    Ok((
        HistoryRequestPayload {
            conversation_token,
            before_osc,
            limit,
            request_id,
            sent_osc,
        },
        sender_pubkey,
    ))
}

/// Build a signed `hist_page` frame carrying the sealed page blob (typically 3–8KB; PT shards larger).
pub fn build_history_page_vsf(
    conversation_token: &[u8; 32],
    request_id: &[u8; 32],
    sealed_blob: Vec<u8>,
    device_pubkey: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<Vec<u8>, String> {
    use vsf::file_format::VsfSection;
    use vsf::VsfBuilder;

    let mut section = VsfSection::new("hist_page");
    section.add_field("tok", VsfType::hg(conversation_token.to_vec()));
    section.add_field("rid", VsfType::hb(request_id.to_vec()));
    let blob_len = sealed_blob.len();
    section.add_field(
        "data",
        VsfType::t_u3(vsf::Tensor::new(vec![blob_len], sealed_blob)),
    );

    let unsigned = VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signature_ed25519(*device_pubkey, [0u8; 64])
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("Failed to build hist_page VSF: {}", e))?;

    vsf::verification::sign_file(unsigned, device_secret)
}

/// Parse + verify a `hist_page` frame. Returns ((conversation_token, request_id, sealed_blob), sender_pubkey). The blob is opaque here; the requester opens it with the friendship history key (AEAD failure = drop).
pub fn parse_history_page_vsf(
    vsf_bytes: &[u8],
) -> Result<(([u8; 32], [u8; 32], Vec<u8>), [u8; 32]), String> {
    let (header, header_end) = vsf::verification::read_verified(vsf_bytes, None)
        .map_err(|e| format!("hist_page verification failed: {}", e))?;
    let sender_pubkey = vsf::verification::extract_signer_pubkey(vsf_bytes)?;

    let (section, section_name) = parse_section_after_header(vsf_bytes, &header, header_end)?;
    if section_name != "hist_page" {
        return Err(format!(
            "Expected 'hist_page' section, got '{}'",
            section_name
        ));
    }
    let fields = &section.fields;

    let conversation_token = field_hash32(fields, "tok", |v| matches!(v, VsfType::hg(_)))
        .ok_or("hist_page missing tok")?;
    let request_id = field_hash32(fields, "rid", |v| matches!(v, VsfType::hb(_)))
        .ok_or("hist_page missing rid")?;
    let sealed = fields
        .iter()
        .find(|f| f.name == "data")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::t_u3(tensor) => Some(tensor.data.clone()),
            _ => None,
        })
        .ok_or("hist_page missing data")?;

    Ok(((conversation_token, request_id, sealed), sender_pubkey))
}

/// Extract a 32-byte hash field of the given VSF flavor from section fields.
fn field_hash32(
    fields: &[vsf::file_format::VsfField],
    name: &str,
    flavor: impl Fn(&VsfType) -> bool,
) -> Option<[u8; 32]> {
    fields
        .iter()
        .find(|f| f.name == name)
        .and_then(|f| f.values.first())
        .filter(|v| flavor(v))
        .and_then(|v| match v {
            VsfType::hg(b) | VsfType::hb(b) | VsfType::hp(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(b);
                Some(arr)
            }
            _ => None,
        })
}

/// Header creation time in eagle oscillations (0 when the header omitted it).
fn header_creation_oscillations(header: &vsf::file_format::VsfHeader) -> i64 {
    match &header.creation_time {
        Some(VsfType::e(vsf::types::EtType::e5(t))) => *t as i64,
        Some(VsfType::e(vsf::types::EtType::e6(t))) => *t,
        Some(VsfType::e(vsf::types::EtType::e7(t))) => *t as i64,
        _ => 0,
    }
}

#[cfg(test)]
mod history_frame_tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn keypair(seed: u8) -> ([u8; 32], [u8; 32]) {
        let sk = SigningKey::from_bytes(&[seed; 32]);
        (sk.verifying_key().to_bytes(), [seed; 32])
    }

    #[test]
    fn hist_req_round_trips() {
        let (pubkey, secret) = keypair(7);
        let tok = [0xA1u8; 32];
        let rid = [0xB2u8; 32];
        let bytes =
            build_history_request_vsf(&tok, i64::MAX, 50, &rid, &pubkey, &secret).unwrap();
        let (payload, signer) = parse_history_request_vsf(&bytes).unwrap();
        assert_eq!(signer, pubkey);
        assert_eq!(payload.conversation_token, tok);
        assert_eq!(payload.request_id, rid);
        assert_eq!(payload.before_osc, i64::MAX);
        assert_eq!(payload.limit, 50);
        assert!(payload.sent_osc > 0);
    }

    #[test]
    fn hist_page_round_trips() {
        let (pubkey, secret) = keypair(9);
        let tok = [0xC3u8; 32];
        let rid = [0xD4u8; 32];
        let blob = vec![0x5Au8; 4096];
        let bytes =
            build_history_page_vsf(&tok, &rid, blob.clone(), &pubkey, &secret).unwrap();
        let ((ptok, prid, psealed), signer) = parse_history_page_vsf(&bytes).unwrap();
        assert_eq!(signer, pubkey);
        assert_eq!(ptok, tok);
        assert_eq!(prid, rid);
        assert_eq!(psealed, blob);
    }

    #[test]
    fn hist_req_bit_flip_rejected() {
        let (pubkey, secret) = keypair(7);
        let mut bytes =
            build_history_request_vsf(&[0xA1u8; 32], 1000, 10, &[0xB2u8; 32], &pubkey, &secret)
                .unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x01;
        assert!(parse_history_request_vsf(&bytes).is_err());
    }
}

#[cfg(test)]
mod phonebook_tests {
    use super::*;
    use crate::types::DevicePubkey;
    use ed25519_dalek::SigningKey;
    use std::net::SocketAddr;

    fn signed_peer(handle: u8, device: u8, ip: &str, last_seen: i64) -> PeerRecord {
        let sk = SigningKey::from_bytes(&[device; 32]);
        let pubkey = DevicePubkey::from_bytes(sk.verifying_key().to_bytes());
        let addr: SocketAddr = ip.parse().unwrap();
        let mut r = PeerRecord::new([handle; 32], pubkey, addr);
        r.last_seen = last_seen;
        r.local_ip = Some("<lan-ip>".parse().unwrap());
        r.sign(&sk);
        assert!(r.verify());
        r
    }

    #[test]
    fn phonebook_request_round_trips() {
        let sk = SigningKey::from_bytes(&[3u8; 32]);
        let msg = FgtwMessage::PhonebookRequest {
            timestamp: 12345,
            sender_pubkey: DevicePubkey::from_bytes(sk.verifying_key().to_bytes()),
            provenance_hash: [0xAB; 32],
            signature: [0xCD; 64],
        };
        let bytes = msg.to_vsf_bytes();
        match FgtwMessage::from_vsf_bytes(&bytes).expect("parse pb_req") {
            FgtwMessage::PhonebookRequest { timestamp, provenance_hash, .. } => {
                assert_eq!(timestamp, 12345);
                assert_eq!(provenance_hash, [0xAB; 32]);
            }
            other => panic!("expected PhonebookRequest, got {:?}", other),
        }
    }

    #[test]
    fn phonebook_response_round_trips_and_peers_still_verify() {
        let peers = vec![
            signed_peer(1, 11, "203.0.113.1:4383", 1000),
            signed_peer(2, 22, "[2001:db8::1]:4383", 2000),
        ];
        let resp = FgtwMessage::PhonebookResponse {
            timestamp: 999,
            responder_pubkey: DevicePubkey::from_bytes([7u8; 32]),
            provenance_hash: [0x11; 32],
            signature: [0x22; 64],
            peers: peers.clone(),
        };
        let bytes = resp.to_vsf_bytes();

        match FgtwMessage::from_vsf_bytes(&bytes).expect("parse pb_resp") {
            FgtwMessage::PhonebookResponse { peers: got, .. } => {
                assert_eq!(got.len(), 2);
                for (a, b) in got.iter().zip(peers.iter()) {
                    assert_eq!(a.handle_proof, b.handle_proof);
                    assert_eq!(a.ip, b.ip);
                    assert_eq!(a.local_ip, b.local_ip);
                    assert_eq!(a.last_seen, b.last_seen);
                    // The signature survived the wire AND still verifies against the embedded pubkey — the whole point: trust travels with the record.
                    assert!(a.verify(), "peer record must still verify after wire round-trip");
                }
            }
            other => panic!("expected PhonebookResponse, got {:?}", other),
        }
    }

    #[test]
    fn avatar_request_round_trips() {
        let sk = SigningKey::from_bytes(&[5u8; 32]);
        let msg = FgtwMessage::AvatarRequest {
            timestamp: 54321,
            sender_pubkey: DevicePubkey::from_bytes(sk.verifying_key().to_bytes()),
            provenance_hash: [0x9A; 32],
            signature: [0xBC; 64],
        };
        let bytes = msg.to_vsf_bytes();
        match FgtwMessage::from_vsf_bytes(&bytes).expect("parse av_req") {
            FgtwMessage::AvatarRequest { timestamp, provenance_hash, .. } => {
                assert_eq!(timestamp, 54321);
                assert_eq!(provenance_hash, [0x9A; 32]);
            }
            other => panic!("expected AvatarRequest, got {:?}", other),
        }
    }

    #[test]
    fn avatar_response_round_trips_with_payload() {
        // A non-trivial payload (a stand-in for the AVIF-in-VSF avatar blob) must survive the wire byte-for-byte.
        let avatar_vsf: Vec<u8> = (0..5000u32).map(|i| (i % 256) as u8).collect();
        let resp = FgtwMessage::AvatarResponse {
            timestamp: 777,
            responder_pubkey: DevicePubkey::from_bytes([4u8; 32]),
            provenance_hash: [0x33; 32],
            signature: [0x44; 64],
            avatar_vsf: avatar_vsf.clone(),
        };
        let bytes = resp.to_vsf_bytes();
        match FgtwMessage::from_vsf_bytes(&bytes).expect("parse av_resp") {
            FgtwMessage::AvatarResponse { timestamp, avatar_vsf: got, .. } => {
                assert_eq!(timestamp, 777);
                assert_eq!(got, avatar_vsf, "avatar payload must round-trip byte-for-byte");
            }
            other => panic!("expected AvatarResponse, got {:?}", other),
        }
    }
}
