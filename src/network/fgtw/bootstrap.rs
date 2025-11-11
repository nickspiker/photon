use super::PeerRecord;
use vsf::parse;

const BOOTSTRAP_URL: &str = "https://fgtw.org/peers.vsf";

/// Load bootstrap peers from fgtw.org/peers.vsf
pub async fn load_bootstrap_peers() -> Result<Vec<PeerRecord>, String> {
    println!("FGTW: Fetching bootstrap peers from {}", BOOTSTRAP_URL);

    // Fetch VSF file
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(BOOTSTRAP_URL)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch bootstrap file: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // Parse VSF peer list
    parse_bootstrap_file(&bytes)
}

/// Parse bootstrap VSF file (list of peer records)
fn parse_bootstrap_file(bytes: &[u8]) -> Result<Vec<PeerRecord>, String> {
    let mut ptr = 0;

    // Parse peer count
    let count = match parse(bytes, &mut ptr).map_err(|e| format!("Parse peer count: {}", e))? {
        vsf::VsfType::u(v, _) => v,
        vsf::VsfType::u3(v) => v as usize,
        vsf::VsfType::u4(v) => v as usize,
        _ => return Err("Invalid peer count type".to_string()),
    };

    let mut peers = Vec::with_capacity(count);
    for _ in 0..count {
        peers.push(parse_peer_record(bytes, &mut ptr)?);
    }

    println!("FGTW: Loaded {} bootstrap peers", peers.len());
    Ok(peers)
}

/// Parse a single peer record from VSF bytes
fn parse_peer_record(bytes: &[u8], ptr: &mut usize) -> Result<PeerRecord, String> {
    use crate::types::PublicIdentity;

    // Parse handle_hash (BLAKE3 hash)
    let hash_bytes = match parse(bytes, ptr).map_err(|e| format!("Parse handle_hash: {}", e))? {
        vsf::VsfType::hb(bytes) => bytes,
        _ => return Err("Invalid handle_hash type".to_string()),
    };
    let mut handle_hash = [0u8; 32];
    handle_hash.copy_from_slice(&hash_bytes);

    // Parse device_pubkey (X25519 key)
    let pubkey_bytes = match parse(bytes, ptr).map_err(|e| format!("Parse device_pubkey: {}", e))? {
        vsf::VsfType::kx(bytes) => bytes,
        _ => return Err("Invalid device_pubkey type".to_string()),
    };
    let mut pubkey_arr = [0u8; 32];
    pubkey_arr.copy_from_slice(&pubkey_bytes);
    let device_pubkey = PublicIdentity::from_bytes(pubkey_arr);

    // Parse IP
    let ip_str = match parse(bytes, ptr).map_err(|e| format!("Parse ip: {}", e))? {
        vsf::VsfType::x(s) => s,
        _ => return Err("Invalid ip type".to_string()),
    };
    let ip = ip_str
        .parse()
        .map_err(|e| format!("Invalid IP address: {}", e))?;

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
