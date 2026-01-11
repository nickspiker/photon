//! FGTW Conduit - Unified relay endpoint
//!
//! All FGTW communication goes through POST /conduit with VSF payloads.
//! Section name in VSF determines operation.

use ed25519_dalek::Signer;
use vsf::VsfType;

use super::Keypair;

const FGTW_URL: &str = "https://fgtw.org";

/// Build a signed VSF for conduit operations
fn build_signed_vsf(
    keypair: &Keypair,
    section_name: &str,
    fields: Vec<(String, VsfType)>,
) -> Result<Vec<u8>, String> {
    // Build unsigned VSF
    let unsigned_bytes = vsf::VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .signed_only(VsfType::ke(keypair.public.as_bytes().to_vec()))
        .add_section(section_name, fields)
        .build()
        .map_err(|e| format!("Build VSF: {}", e))?;

    // Compute provenance hash and sign
    let hash_bytes = vsf::verification::compute_provenance_hash(&unsigned_bytes)
        .map_err(|e| format!("Compute hash: {}", e))?;

    let signature = keypair.sign(&hash_bytes);

    // Fill in hp and ge fields
    let mut signed_bytes = unsigned_bytes;
    vsf::verification::fill_provenance_hash(&mut signed_bytes, &hash_bytes)
        .map_err(|e| format!("Fill hash: {}", e))?;
    vsf::verification::fill_signature(&mut signed_bytes, &signature.to_bytes())
        .map_err(|e| format!("Fill signature: {}", e))?;

    Ok(signed_bytes)
}

/// Send a message via FGTW conduit relay
///
/// # Arguments
/// * `keypair` - Our device keypair for signing
/// * `recipient_pubkey` - Recipient's device public key (32 bytes)
/// * `message_bytes` - Already-encrypted message (VSF format)
///
/// # Returns
/// Ok(()) on success, Err with message on failure
pub async fn send_via_relay(
    keypair: &Keypair,
    recipient_pubkey: &[u8; 32],
    message_bytes: &[u8],
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    // Build relay VSF
    let vsf_bytes = build_signed_vsf(
        keypair,
        "relay",
        vec![
            ("recipient".to_string(), VsfType::kx(recipient_pubkey.to_vec())),
            ("payload".to_string(), VsfType::v(b'r', message_bytes.to_vec())),
        ],
    )?;

    let response = client
        .post(&format!("{}/conduit", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .await
        .map_err(|e| format!("Failed to send relay: {}", e))?;

    let status = response.status();
    if status.is_success() {
        crate::log(&format!(
            "RELAY: Stored message for {}...",
            hex::encode(&recipient_pubkey[..4])
        ));
        Ok(())
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(format!("Relay failed ({}): {}", status, body))
    }
}

/// Fetch pending messages from FGTW conduit
///
/// # Arguments
/// * `keypair` - Our device keypair for authentication
///
/// # Returns
/// Concatenated VSF messages (each self-delimiting via L field)
pub async fn fetch_relay_messages(keypair: &Keypair) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    // Build fetch VSF (no fields needed, identity proven by signature)
    let vsf_bytes = build_signed_vsf(keypair, "fetch", vec![])?;

    let response = client
        .post(&format!("{}/conduit", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch relay: {}", e))?;

    let status = response.status();
    if status.is_success() {
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read body: {}", e))?;

        // Response is VSF with section "fetched" containing "messages" field
        // Parse to extract the raw messages
        if bytes.is_empty() {
            return Ok(Vec::new());
        }

        // Parse response VSF to extract messages
        use vsf::file_format::{VsfHeader, VsfSection};
        let (_, header_end) = VsfHeader::decode(&bytes)
            .map_err(|e| format!("Parse response header: {}", e))?;

        let mut ptr = header_end;
        let section = VsfSection::parse(&bytes, &mut ptr)
            .map_err(|e| format!("Parse fetched section: {}", e))?;

        let messages = section.get_field("messages")
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::v(_, data) => Some(data.clone()),
                _ => None,
            })
            .unwrap_or_default();

        if !messages.is_empty() {
            crate::log(&format!("RELAY: Fetched {} bytes", messages.len()));
        }

        Ok(messages)
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(format!("Fetch failed ({}): {}", status, body))
    }
}

/// Synchronous version of send_via_relay for non-async contexts
pub fn send_via_relay_sync(
    keypair: &Keypair,
    recipient_pubkey: &[u8; 32],
    message_bytes: &[u8],
) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    // Build relay VSF
    let vsf_bytes = build_signed_vsf(
        keypair,
        "relay",
        vec![
            ("recipient".to_string(), VsfType::kx(recipient_pubkey.to_vec())),
            ("payload".to_string(), VsfType::v(b'r', message_bytes.to_vec())),
        ],
    )?;

    let response = client
        .post(&format!("{}/conduit", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| format!("Failed to send relay: {}", e))?;

    let status = response.status();
    if status.is_success() {
        crate::log(&format!(
            "RELAY: Stored message for {}...",
            hex::encode(&recipient_pubkey[..4])
        ));
        Ok(())
    } else {
        let body = response.text().unwrap_or_default();
        Err(format!("Relay failed ({}): {}", status, body))
    }
}

/// Synchronous version of fetch_relay_messages for non-async contexts
pub fn fetch_relay_messages_sync(keypair: &Keypair) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    // Build fetch VSF
    let vsf_bytes = build_signed_vsf(keypair, "fetch", vec![])?;

    let response = client
        .post(&format!("{}/conduit", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| format!("Failed to fetch relay: {}", e))?;

    let status = response.status();
    if status.is_success() {
        let bytes = response
            .bytes()
            .map_err(|e| format!("Failed to read body: {}", e))?;

        if bytes.is_empty() {
            return Ok(Vec::new());
        }

        // Parse response VSF to extract messages
        use vsf::file_format::{VsfHeader, VsfSection};
        let (_, header_end) = VsfHeader::decode(&bytes)
            .map_err(|e| format!("Parse response header: {}", e))?;

        let mut ptr = header_end;
        let section = VsfSection::parse(&bytes, &mut ptr)
            .map_err(|e| format!("Parse fetched section: {}", e))?;

        let messages = section.get_field("messages")
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::v(_, data) => Some(data.clone()),
                _ => None,
            })
            .unwrap_or_default();

        if !messages.is_empty() {
            crate::log(&format!("RELAY: Fetched {} bytes", messages.len()));
        }

        Ok(messages)
    } else {
        let body = response.text().unwrap_or_default();
        Err(format!("Fetch failed ({}): {}", status, body))
    }
}
