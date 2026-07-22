//! FGTW Conduit - Unified relay endpoint
//!
//! All FGTW communication goes thru POST /conduit with VSF payloads. Section name in VSF determines operation.

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
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only(VsfType::ke(keypair.public.as_bytes().to_vec()))
        .add_section(section_name, fields)
        .build()
        .map_err(|e| format!("Build VSF: {}", e))?;

    // Canonical vsf signing (fills hp, then ge over BLAKE3(file, ge zeroed)) — matches the scheme the worker verifies. The old code signed the bare hp value, which the worker's file-hash verification REJECTED, so every relay send died with bad_signature (masked because relay is a non-fatal last-resort fallback).
    vsf::verification::sign_file(unsigned_bytes, keypair.secret.as_bytes())
}

/// Send a message via FGTW conduit relay
///
/// # Arguments
/// * `keypair` - Our device keypair for signing * `recipient_pubkey` - Recipient's device public key (32 bytes) * `message_bytes` - Already-encrypted message (VSF format)
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
            (
                "recipient".to_string(),
                VsfType::kx(recipient_pubkey.to_vec()),
            ),
            (
                "payload".to_string(),
                VsfType::v(b'r', message_bytes.to_vec()),
            ),
        ],
    )?;

    let response = client
        .post(&format!("{}", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .await
        .map_err(|e| format!("Failed to send relay: {}", e))?;

    let status = response.status();
    let body = response.bytes().await.unwrap_or_default();
    if let Some((reason, detail)) = fgtw::client::error_frame(&body) {
        return Err(format!("Relay failed ({reason}): {detail}"));
    }
    if !status.is_success() {
        return Err(format!("Relay failed (transport {})", status));
    }
    crate::logf!("RELAY: Stored message for {}...", hex::encode(&recipient_pubkey[..4]));
    Ok(())
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
            (
                "recipient".to_string(),
                VsfType::kx(recipient_pubkey.to_vec()),
            ),
            (
                "payload".to_string(),
                VsfType::v(b'r', message_bytes.to_vec()),
            ),
        ],
    )?;

    let response = client
        .post(&format!("{}", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| format!("Failed to send relay: {}", e))?;

    let status = response.status();
    let body = response.bytes().unwrap_or_default();
    if let Some((reason, detail)) = fgtw::client::error_frame(&body) {
        return Err(format!("Relay failed ({reason}): {detail}"));
    }
    if !status.is_success() {
        return Err(format!("Relay failed (transport {})", status));
    }
    crate::logf!("RELAY: Stored message for {}...", hex::encode(&recipient_pubkey[..4]));
    Ok(())
}

