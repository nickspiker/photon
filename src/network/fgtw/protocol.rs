use crate::types::PublicIdentity;
use std::net::{IpAddr, SocketAddr};
use vsf::schema::FromVsfType;
use vsf::types::Vector;
use vsf::VsfType;

/// FGTW protocol messages (VSF serialized)
#[derive(Debug, Clone)]
pub enum FgtwMessage {
    Ping {
        device_pubkey: PublicIdentity,
    },
    Pong {
        device_pubkey: PublicIdentity,
        peers: Vec<PeerRecord>,
    },
    FindNode {
        handle_hash: [u8; 32],
        requester_pubkey: PublicIdentity,
    },
    FoundNodes {
        devices: Vec<PeerRecord>,
    },
    Announce {
        handle_hash: [u8; 32],
        device_pubkey: PublicIdentity,
        port: u16,
    },
    Query {
        handle_hash: [u8; 32],
        requester_pubkey: PublicIdentity,
    },
    QueryResponse {
        devices: Vec<PeerRecord>,
    },
}

/// Peer record - one device for a user handle
#[derive(Debug, Clone)]
pub struct PeerRecord {
    pub handle_hash: [u8; 32],         // BLAKE3 of VSF-normalized username
    pub device_pubkey: PublicIdentity, // Device's X25519 public key (used as device identifier)
    pub ip: SocketAddr,                // Where to reach this device
    pub last_seen: f64,                // Timestamp (f64, serializes as VSF type f6)
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
            bytes[0], bytes[1], bytes[2], bytes[3]
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
            FgtwMessage::Ping { device_pubkey } => {
                builder.add_section("fgtw", vec![
                    ("msg_type".to_string(), VsfType::u3(0)),
                    ("device_pubkey".to_string(), VsfType::kx(device_pubkey.as_bytes().to_vec())),
                ]).build()
            }
            FgtwMessage::Pong { device_pubkey, peers } => {
                let mut fields = vec![
                    ("msg_type".to_string(), VsfType::u3(1)),
                    ("device_pubkey".to_string(), VsfType::kx(device_pubkey.as_bytes().to_vec())),
                    ("peer_count".to_string(), VsfType::u(peers.len(), false)),
                ];

                // Add each peer as separate fields
                for (i, peer) in peers.iter().enumerate() {
                    let prefix = format!("peer_{}", i);
                    fields.push((format!("{}_handle_hash", prefix), VsfType::hb(peer.handle_hash.to_vec())));
                    fields.push((format!("{}_device_pubkey", prefix), VsfType::kx(peer.device_pubkey.as_bytes().to_vec())));
                    fields.push((format!("{}_ip", prefix), VsfType::v_u3(Vector { data: socketaddr_to_bytes(&peer.ip) })));
                    fields.push((format!("{}_last_seen", prefix), VsfType::f6(peer.last_seen)));
                }

                builder.add_section("fgtw", fields).build()
            }
            FgtwMessage::FindNode { handle_hash, requester_pubkey } => {
                builder.add_section("fgtw", vec![
                    ("msg_type".to_string(), VsfType::u3(2)),
                    ("handle_hash".to_string(), VsfType::hb(handle_hash.to_vec())),
                    ("requester_pubkey".to_string(), VsfType::kx(requester_pubkey.as_bytes().to_vec())),
                ]).build()
            }
            FgtwMessage::FoundNodes { devices } => {
                let mut fields = vec![
                    ("msg_type".to_string(), VsfType::u3(3)),
                    ("device_count".to_string(), VsfType::u(devices.len(), false)),
                ];

                for (i, device) in devices.iter().enumerate() {
                    let prefix = format!("device_{}", i);
                    fields.push((format!("{}_handle_hash", prefix), VsfType::hb(device.handle_hash.to_vec())));
                    fields.push((format!("{}_device_pubkey", prefix), VsfType::kx(device.device_pubkey.as_bytes().to_vec())));
                    fields.push((format!("{}_ip", prefix), VsfType::v_u3(Vector { data: socketaddr_to_bytes(&device.ip) })));
                    fields.push((format!("{}_last_seen", prefix), VsfType::f6(device.last_seen)));
                }

                builder.add_section("fgtw", fields).build()
            }
            FgtwMessage::Announce { handle_hash, device_pubkey, port } => {
                builder.add_section("fgtw", vec![
                    ("msg_type".to_string(), VsfType::u3(4)),
                    ("handle_hash".to_string(), VsfType::hb(handle_hash.to_vec())),
                    ("device_pubkey".to_string(), VsfType::kx(device_pubkey.as_bytes().to_vec())),
                    ("port".to_string(), VsfType::u(*port as usize, false)),
                ]).build()
            }
            FgtwMessage::Query { handle_hash, requester_pubkey } => {
                builder.add_section("fgtw", vec![
                    ("msg_type".to_string(), VsfType::u3(5)),
                    ("handle_hash".to_string(), VsfType::hb(handle_hash.to_vec())),
                    ("requester_pubkey".to_string(), VsfType::kx(requester_pubkey.as_bytes().to_vec())),
                ]).build()
            }
            FgtwMessage::QueryResponse { devices } => {
                let mut fields = vec![
                    ("msg_type".to_string(), VsfType::u3(6)),
                    ("device_count".to_string(), VsfType::u(devices.len(), false)),
                ];

                for (i, device) in devices.iter().enumerate() {
                    let prefix = format!("device_{}", i);
                    fields.push((format!("{}_handle_hash", prefix), VsfType::hb(device.handle_hash.to_vec())));
                    fields.push((format!("{}_device_pubkey", prefix), VsfType::kx(device.device_pubkey.as_bytes().to_vec())));
                    fields.push((format!("{}_ip", prefix), VsfType::v_u3(Vector { data: socketaddr_to_bytes(&device.ip) })));
                    fields.push((format!("{}_last_seen", prefix), VsfType::f6(device.last_seen)));
                }

                builder.add_section("fgtw", fields).build()
            }
        };

        result.unwrap_or_else(|e| {
            eprintln!("FGTW: Failed to build VSF message: {}", e);
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

        // Parse VSF file to find the "fgtw" section
        use vsf::parse;

        let mut ptr = 0;

        // Skip magic "RÅ<"
        ptr = 4;

        // Parse header length
        let _header_length = match parse(bytes, &mut ptr) {
            Ok(VsfType::b(len, _)) => len,
            _ => return Err("Invalid header length".to_string()),
        };

        // Skip version, backward_compat, creation_time, hashes
        // Just scan forward to find the section marker '>'
        while ptr < bytes.len() && bytes[ptr] != b'>' {
            ptr += 1;
        }

        if ptr >= bytes.len() {
            return Err("No header end marker found".to_string());
        }

        ptr += 1; // Skip '>'

        // Now we should be at the section start '['
        if ptr >= bytes.len() || bytes[ptr] != b'[' {
            return Err("No section found".to_string());
        }

        ptr += 1; // Skip '['

        // Parse section name (should be "fgtw")
        let section_name = match parse(bytes, &mut ptr) {
            Ok(VsfType::d(name)) => name,
            _ => return Err("Invalid section name".to_string()),
        };

        if section_name != "fgtw" {
            return Err(format!("Expected 'fgtw' section, got '{}'", section_name));
        }

        // Parse fields into a map
        use std::collections::HashMap;
        let mut fields: HashMap<String, VsfType> = HashMap::new();

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
                let value = parse(bytes, &mut ptr).map_err(|e| format!("Parse field value: {}", e))?;
                fields.insert(field_name, value);
            }

            // Skip closing ')'
            if ptr >= bytes.len() || bytes[ptr] != b')' {
                return Err("Expected field end ')'".to_string());
            }
            ptr += 1;
        }

        // Extract msg_type
        let msg_type = match fields.get("msg_type") {
            Some(vsf_val) => u8::from_vsf_type(vsf_val)
                .map_err(|e| format!("Invalid msg_type: {}", e))?,
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
                Ok(FgtwMessage::Pong { device_pubkey, peers })
            }
            2 => {
                // FindNode
                let handle_hash = extract_hash(&fields, "handle_hash")?;
                let requester_pubkey = extract_pubkey(&fields, "requester_pubkey")?;
                Ok(FgtwMessage::FindNode { handle_hash, requester_pubkey })
            }
            3 => {
                // FoundNodes
                let devices = extract_peer_list(&fields, "device")?;
                Ok(FgtwMessage::FoundNodes { devices })
            }
            4 => {
                // Announce
                let handle_hash = extract_hash(&fields, "handle_hash")?;
                let device_pubkey = extract_pubkey(&fields, "device_pubkey")?;
                let port = match fields.get("port") {
                    Some(vsf_val) => u16::from_vsf_type(vsf_val)
                        .map_err(|e| format!("Invalid port: {}", e))?,
                    None => return Err("Missing port".to_string()),
                };
                Ok(FgtwMessage::Announce { handle_hash, device_pubkey, port })
            }
            5 => {
                // Query
                let handle_hash = extract_hash(&fields, "handle_hash")?;
                let requester_pubkey = extract_pubkey(&fields, "requester_pubkey")?;
                Ok(FgtwMessage::Query { handle_hash, requester_pubkey })
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
    pub fn new(handle_hash: [u8; 32], device_pubkey: PublicIdentity, ip: SocketAddr) -> Self {
        Self {
            handle_hash,
            device_pubkey,
            ip,
            last_seen: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64(),
        }
    }
}

// Helper functions for extracting fields from VSF
fn extract_hash(fields: &std::collections::HashMap<String, VsfType>, key: &str) -> Result<[u8; 32], String> {
    let hash_bytes = match fields.get(key) {
        Some(VsfType::hb(bytes)) => bytes,
        _ => return Err(format!("Missing or invalid hash: {}", key)),
    };
    let mut arr = [0u8; 32];
    if hash_bytes.len() != 32 {
        return Err(format!("Hash {} must be 32 bytes", key));
    }
    arr.copy_from_slice(&hash_bytes);
    Ok(arr)
}

fn extract_pubkey(fields: &std::collections::HashMap<String, VsfType>, key: &str) -> Result<PublicIdentity, String> {
    let pubkey_bytes = match fields.get(key) {
        Some(VsfType::kx(bytes)) => bytes,
        _ => return Err(format!("Missing or invalid pubkey: {}", key)),
    };
    let mut pubkey_arr = [0u8; 32];
    if pubkey_bytes.len() != 32 {
        return Err(format!("Pubkey {} must be 32 bytes", key));
    }
    pubkey_arr.copy_from_slice(&pubkey_bytes);
    Ok(PublicIdentity::from_bytes(pubkey_arr))
}

fn extract_peer_list(fields: &std::collections::HashMap<String, VsfType>, prefix: &str) -> Result<Vec<PeerRecord>, String> {
    let count_key = format!("{}_count", prefix);
    let count = match fields.get(&count_key) {
        Some(vsf_val) => usize::from_vsf_type(vsf_val)
            .map_err(|e| format!("Invalid {}_count: {}", prefix, e))?,
        None => return Err(format!("Missing {}_count", prefix)),
    };

    let mut peers = Vec::with_capacity(count);
    for i in 0..count {
        let peer_prefix = format!("{}_{}", prefix, i);

        let handle_hash = extract_hash(fields, &format!("{}_handle_hash", peer_prefix))?;
        let device_pubkey = extract_pubkey(fields, &format!("{}_device_pubkey", peer_prefix))?;

        let ip_key = format!("{}_ip", peer_prefix);
        let ip_bytes = match fields.get(&ip_key) {
            Some(VsfType::v_u3(vec)) => &vec.data,
            _ => return Err(format!("Missing or invalid {}", ip_key)),
        };
        let ip = bytes_to_socketaddr(ip_bytes)
            .ok_or_else(|| format!("Invalid IP bytes for {}", ip_key))?;

        let last_seen_key = format!("{}_last_seen", peer_prefix);
        let last_seen = match fields.get(&last_seen_key) {
            Some(VsfType::f6(v)) => *v,
            _ => return Err(format!("Missing or invalid {}", last_seen_key)),
        };

        peers.push(PeerRecord {
            handle_hash,
            device_pubkey,
            ip,
            last_seen,
        });
    }

    Ok(peers)
}
