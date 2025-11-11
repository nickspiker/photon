use crate::types::PublicIdentity;
use std::net::SocketAddr;
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

impl FgtwMessage {
    /// Serialize to VSF bytes
    pub fn to_vsf_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        match self {
            FgtwMessage::Ping { device_pubkey } => {
                bytes.extend(VsfType::u3(0).flatten());
                bytes.extend(VsfType::kx(device_pubkey.as_bytes().to_vec()).flatten());
            }
            FgtwMessage::Pong {
                device_pubkey,
                peers,
            } => {
                bytes.extend(VsfType::u3(1).flatten());
                bytes.extend(VsfType::kx(device_pubkey.as_bytes().to_vec()).flatten());
                bytes.extend(serialize_peer_list(peers));
            }
            FgtwMessage::FindNode {
                handle_hash,
                requester_pubkey,
            } => {
                bytes.extend(VsfType::u3(2).flatten());
                bytes.extend(VsfType::hb(handle_hash.to_vec()).flatten());
                bytes.extend(VsfType::kx(requester_pubkey.as_bytes().to_vec()).flatten());
            }
            FgtwMessage::FoundNodes { devices } => {
                bytes.extend(VsfType::u3(3).flatten());
                bytes.extend(serialize_peer_list(devices));
            }
            FgtwMessage::Announce {
                handle_hash,
                device_pubkey,
                port,
            } => {
                bytes.extend(VsfType::u3(4).flatten());
                bytes.extend(VsfType::hb(handle_hash.to_vec()).flatten());
                bytes.extend(VsfType::kx(device_pubkey.as_bytes().to_vec()).flatten());
                bytes.extend(VsfType::u(*port as usize, false).flatten());
            }
            FgtwMessage::Query {
                handle_hash,
                requester_pubkey,
            } => {
                bytes.extend(VsfType::u3(5).flatten());
                bytes.extend(VsfType::hb(handle_hash.to_vec()).flatten());
                bytes.extend(VsfType::kx(requester_pubkey.as_bytes().to_vec()).flatten());
            }
            FgtwMessage::QueryResponse { devices } => {
                bytes.extend(VsfType::u3(6).flatten());
                bytes.extend(serialize_peer_list(devices));
            }
        }

        bytes
    }

    /// Deserialize from VSF bytes
    pub fn from_vsf_bytes(bytes: &[u8]) -> Result<Self, String> {
        use vsf::parse;

        let mut ptr = 0;
        let msg_type = match parse(bytes, &mut ptr).map_err(|e| format!("Parse msg type: {}", e))? {
            VsfType::u3(v) => v,
            _ => return Err("Invalid message type".to_string()),
        };

        match msg_type {
            0 => {
                // Ping
                let device_pubkey = parse_pubkey(bytes, &mut ptr)?;
                Ok(FgtwMessage::Ping { device_pubkey })
            }
            1 => {
                // Pong
                let device_pubkey = parse_pubkey(bytes, &mut ptr)?;
                let peers = parse_peer_list(bytes, &mut ptr)?;
                Ok(FgtwMessage::Pong {
                    device_pubkey,
                    peers,
                })
            }
            2 => {
                // FindNode
                let handle_hash = parse_hash(bytes, &mut ptr)?;
                let requester_pubkey = parse_pubkey(bytes, &mut ptr)?;
                Ok(FgtwMessage::FindNode {
                    handle_hash,
                    requester_pubkey,
                })
            }
            3 => {
                // FoundNodes
                let devices = parse_peer_list(bytes, &mut ptr)?;
                Ok(FgtwMessage::FoundNodes { devices })
            }
            4 => {
                // Announce
                let handle_hash = parse_hash(bytes, &mut ptr)?;
                let device_pubkey = parse_pubkey(bytes, &mut ptr)?;

                let port = match parse(bytes, &mut ptr).map_err(|e| format!("Parse port: {}", e))? {
                    VsfType::u(v, _) => v as u16,
                    VsfType::u3(v) => v as u16,
                    VsfType::u4(v) => v as u16,
                    _ => return Err("Invalid port type".to_string()),
                };
                Ok(FgtwMessage::Announce {
                    handle_hash,
                    device_pubkey,
                    port,
                })
            }
            5 => {
                // Query
                let handle_hash = parse_hash(bytes, &mut ptr)?;
                let requester_pubkey = parse_pubkey(bytes, &mut ptr)?;
                Ok(FgtwMessage::Query {
                    handle_hash,
                    requester_pubkey,
                })
            }
            6 => {
                // QueryResponse
                let devices = parse_peer_list(bytes, &mut ptr)?;
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

// Helper functions for serialization
fn serialize_peer_record(peer: &PeerRecord) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(VsfType::hb(peer.handle_hash.to_vec()).flatten());
    bytes.extend(VsfType::kx(peer.device_pubkey.as_bytes().to_vec()).flatten());
    bytes.extend(VsfType::x(peer.ip.to_string()).flatten());
    bytes.extend(VsfType::f6(peer.last_seen).flatten());
    bytes
}

fn serialize_peer_list(peers: &[PeerRecord]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(VsfType::u(peers.len(), false).flatten());
    for peer in peers {
        bytes.extend(serialize_peer_record(peer));
    }
    bytes
}

fn parse_hash(bytes: &[u8], ptr: &mut usize) -> Result<[u8; 32], String> {
    use vsf::parse;
    let hash_bytes = match parse(bytes, ptr).map_err(|e| format!("Parse hash: {}", e))? {
        VsfType::hb(bytes) => bytes,
        _ => return Err("Invalid hash type".to_string()),
    };
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&hash_bytes);
    Ok(arr)
}

fn parse_pubkey(bytes: &[u8], ptr: &mut usize) -> Result<PublicIdentity, String> {
    use vsf::parse;
    let pubkey_bytes = match parse(bytes, ptr).map_err(|e| format!("Parse pubkey: {}", e))? {
        VsfType::kx(bytes) => bytes,
        _ => return Err("Invalid pubkey type".to_string()),
    };
    let mut pubkey_arr = [0u8; 32];
    pubkey_arr.copy_from_slice(&pubkey_bytes);
    Ok(PublicIdentity::from_bytes(pubkey_arr))
}

fn parse_peer_record(bytes: &[u8], ptr: &mut usize) -> Result<PeerRecord, String> {
    use vsf::parse;

    let handle_hash = parse_hash(bytes, ptr)?;
    let device_pubkey = parse_pubkey(bytes, ptr)?;

    let ip_str = match parse(bytes, ptr).map_err(|e| format!("Parse ip: {}", e))? {
        VsfType::x(s) => s,
        _ => return Err("Invalid ip type".to_string()),
    };
    let ip: SocketAddr = ip_str.parse().map_err(|e| format!("Invalid IP: {}", e))?;

    let last_seen = match parse(bytes, ptr).map_err(|e| format!("Parse last_seen: {}", e))? {
        VsfType::f6(v) => v,
        _ => return Err("Invalid last_seen type".to_string()),
    };

    Ok(PeerRecord {
        handle_hash,
        device_pubkey,
        ip,
        last_seen,
    })
}

fn parse_peer_list(bytes: &[u8], ptr: &mut usize) -> Result<Vec<PeerRecord>, String> {
    use vsf::parse;

    let count = match parse(bytes, ptr).map_err(|e| format!("Parse peer count: {}", e))? {
        VsfType::u(v, _) => v,
        VsfType::u3(v) => v as usize,
        VsfType::u4(v) => v as usize,
        _ => return Err("Invalid peer count type".to_string()),
    };

    let mut peers = Vec::with_capacity(count);
    for _ in 0..count {
        peers.push(parse_peer_record(bytes, ptr)?);
    }
    Ok(peers)
}
