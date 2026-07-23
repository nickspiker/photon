//! FGTW Conduit - Unified relay endpoint
//!
//! All FGTW communication goes thru POST /conduit with VSF payloads. Section name in VSF determines operation.

use vsf::VsfType;

use super::Keypair;

const FGTW_URL: &str = "https://fgtw.org";

/// Peel a relay envelope received over the pipe: the whole signed `relay` VSF the SENDER built
/// (`build_signed_vsf("relay", {recipient, payload})`, signed with their device key), which the worker now
/// forwards intact instead of the unwrapped inner. Verifies the sender's whole-file signature, then returns
/// `(sender_device_key, inner_payload)`. `None` on any structural/parse/verify failure — a malformed or
/// unsigned frame off the pipe is dropped, never injected. This is the DOMAIN SEPARATOR for the pipe: a
/// message is known-relayed because it arrived wrapped in this authenticated envelope, not because of a
/// sentinel address. The inner payload is byte-identical to a direct message, so no inner parser changes.
pub fn peel_relay_envelope(bytes: &[u8]) -> Option<([u8; 32], Vec<u8>)> {
    use vsf::file_format::{VsfHeader, VsfSection};

    // Verify the sender's whole-file signature with `verify_file_signature`, NOT `read_verified`.
    // `read_verified` additionally enforces `is_original` (the header `hp` must equal the content hash), but
    // build_signed_vsf uses `signed_only(ke)` + `sign_file`, and the same waiver every CLUTCH/chat parser
    // takes applies here: the signature covers the ENTIRE file (authorship + integrity are proven), only the
    // content-hp self-attestation is not asserted. Using read_verified rejected every real envelope — the
    // "not a valid signed relay envelope" drop that black-holed the whole pipe data plane.
    match vsf::verification::verify_file_signature(bytes) {
        Ok(true) => {}
        _ => return None,
    }
    let (header, header_end) = VsfHeader::decode(bytes).ok()?;

    // Signer device key from the header (the sender that built + signed this envelope).
    let sender_key: [u8; 32] = match &header.signer_pubkey {
        Some(VsfType::ke(k)) if k.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(k);
            arr
        }
        _ => return None,
    };

    // Resolve the section via `primary_section`, NOT a bare `VsfSection::parse`: the section NAME lives in the
    // header TOC (near-form), so a body parse returns `section.name == ""` and a hand-rolled `== "relay"` check
    // silently fails — the trap that black-holed the pipe (and, historically, the hub push accelerator). Pull
    // the inner payload (v'r') off the resolved primary section.
    let section = header.primary_section(bytes, header_end).ok()?;
    let payload = section
        .get_field("payload")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::v(_, data) => Some(data.clone()),
            _ => None,
        })?;
    if payload.is_empty() {
        return None;
    }
    Some((sender_key, payload))
}

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

#[cfg(test)]
mod peel_tests {
    use super::*;

    /// A `send_via_relay` envelope must round-trip through `peel_relay_envelope`. This guards the TOC-name trap
    /// specifically: the section name lives in the header near-form, so a bare body parse sees `name == ""` and
    /// any `== "relay"` check fails — which silently black-holed the whole pipe data plane until caught in a log.
    #[test]
    fn peel_roundtrip() {
        let kp = crate::network::fgtw::Keypair::from_seed(&[3u8; 32]);
        let inner = vec![9u8; 179];
        let envelope = build_signed_vsf(
            &kp,
            "relay",
            vec![
                ("recipient".to_string(), VsfType::kx([7u8; 32].to_vec())),
                ("payload".to_string(), VsfType::v(b'r', inner.clone())),
            ],
        )
        .expect("build envelope");
        let (sender, payload) = peel_relay_envelope(&envelope).expect("peel must succeed");
        assert_eq!(sender, kp.public.to_bytes(), "sender key must be the signer");
        assert_eq!(payload, inner, "inner payload must round-trip byte-identical");
    }
}

