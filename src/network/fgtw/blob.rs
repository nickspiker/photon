use super::fingerprint::Keypair;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

const FGTW_URL: &str = "https://fgtw.org";

// ============================================================================
// Blob Storage API (GET/PUT/DELETE)
// ============================================================================

/// Error type for blob operations
#[derive(Debug)]
pub enum BlobError {
    Network(String),
    NotFound,
    Unauthorized(String),
    ServerError(String),
}

impl std::fmt::Display for BlobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlobError::Network(s) => write!(f, "Network error: {}", s),
            BlobError::NotFound => write!(f, "Blob not found"),
            BlobError::Unauthorized(s) => write!(f, "Unauthorized: {}", s),
            BlobError::ServerError(s) => write!(f, "Server error: {}", s),
        }
    }
}

/// Upload a blob to FGTW storage
///
/// # Arguments
/// * `storage_key` - Base64url-encoded 32-byte key (43 chars)
/// * `data` - Raw bytes to store (already encrypted by caller)
/// * `device_keypair` - Ed25519 keypair for signing
/// * `handle_proof` - 32-byte handle proof (proves registered user)
///
/// # Auth Headers
/// * X-Device-Pubkey: base64url(ed25519_pubkey)
/// * X-Signature: base64url(sign(storage_key_bytes))
/// * X-Timestamp: f64 Eagle Time nanoseconds
/// * X-Handle-Proof: base64url(handle_proof) - proves registered peer
pub async fn put_blob(
    storage_key: &str,
    data: &[u8],
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
) -> Result<(), BlobError> {
    use ed25519_dalek::Signer;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    // Decode storage key to bytes for signing
    let key_bytes = URL_SAFE_NO_PAD
        .decode(storage_key.as_bytes())
        .map_err(|e| BlobError::Network(format!("Invalid storage key: {}", e)))?;

    // Sign the storage key bytes
    let signature = device_keypair.secret.sign(&key_bytes);

    // Current Eagle Time for replay protection
    let timestamp = vsf::eagle_time_nanos();

    // Encode headers
    let pubkey_b64 = URL_SAFE_NO_PAD.encode(device_keypair.public.as_bytes());
    let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());
    let handle_proof_b64 = URL_SAFE_NO_PAD.encode(handle_proof);

    let response = client
        .put(&format!("{}/blob/{}", FGTW_URL, storage_key))
        .header("Content-Type", "application/octet-stream")
        .header("X-Device-Pubkey", pubkey_b64)
        .header("X-Signature", signature_b64)
        .header("X-Timestamp", format!("{:.0}", timestamp))
        .header("X-Handle-Proof", handle_proof_b64)
        .body(data.to_vec())
        .send()
        .await
        .map_err(|e| BlobError::Network(format!("PUT request failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        crate::log_info(&format!("FGTW: Uploaded blob ({} bytes)", data.len()));
        Ok(())
    } else if status == reqwest::StatusCode::FORBIDDEN {
        let body = response.text().await.unwrap_or_default();
        Err(BlobError::Unauthorized(body))
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}

/// Download a blob from FGTW storage (unauthenticated read)
///
/// # Arguments
/// * `storage_key` - Base64url-encoded 32-byte key (43 chars)
///
/// # Returns
/// * `Ok(Some(bytes))` - Blob data
/// * `Ok(None)` - Blob not found (404)
/// * `Err(...)` - Other error
pub async fn get_blob(storage_key: &str) -> Result<Option<Vec<u8>>, BlobError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    let response = client
        .get(&format!("{}/blob/{}", FGTW_URL, storage_key))
        .send()
        .await
        .map_err(|e| BlobError::Network(format!("GET request failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        let bytes = response
            .bytes()
            .await
            .map_err(|e| BlobError::Network(format!("Failed to read blob: {}", e)))?;
        crate::log_info(&format!("FGTW: Downloaded blob ({} bytes)", bytes.len()));
        Ok(Some(bytes.to_vec()))
    } else if status == reqwest::StatusCode::NOT_FOUND {
        Ok(None)
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}

/// Upload a blob to FGTW storage (blocking version)
///
/// Same as put_blob but uses blocking HTTP client for sync contexts
pub fn put_blob_blocking(
    storage_key: &str,
    data: &[u8],
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
) -> Result<(), BlobError> {
    use ed25519_dalek::Signer;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    // Decode storage key to bytes for signing
    let key_bytes = URL_SAFE_NO_PAD
        .decode(storage_key.as_bytes())
        .map_err(|e| BlobError::Network(format!("Invalid storage key: {}", e)))?;

    // Sign the storage key bytes
    let signature = device_keypair.secret.sign(&key_bytes);

    // Current Eagle Time for replay protection
    let timestamp = vsf::eagle_time_nanos();

    // Encode headers
    let pubkey_b64 = URL_SAFE_NO_PAD.encode(device_keypair.public.as_bytes());
    let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());
    let handle_proof_b64 = URL_SAFE_NO_PAD.encode(handle_proof);

    let response = client
        .put(&format!("{}/blob/{}", FGTW_URL, storage_key))
        .header("Content-Type", "application/octet-stream")
        .header("X-Device-Pubkey", pubkey_b64)
        .header("X-Signature", signature_b64)
        .header("X-Timestamp", format!("{:.0}", timestamp))
        .header("X-Handle-Proof", handle_proof_b64)
        .body(data.to_vec())
        .send()
        .map_err(|e| BlobError::Network(format!("PUT request failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        crate::log_info(&format!("FGTW: Uploaded blob ({} bytes)", data.len()));
        Ok(())
    } else if status == reqwest::StatusCode::FORBIDDEN {
        let body = response.text().unwrap_or_default();
        Err(BlobError::Unauthorized(body))
    } else {
        let body = response.text().unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}

/// Download a blob from FGTW storage (blocking version)
///
/// Same as get_blob but uses blocking HTTP client for sync contexts
pub fn get_blob_blocking(storage_key: &str) -> Result<Option<Vec<u8>>, BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    let response = client
        .get(&format!("{}/blob/{}", FGTW_URL, storage_key))
        .send()
        .map_err(|e| BlobError::Network(format!("GET request failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        let bytes = response
            .bytes()
            .map_err(|e| BlobError::Network(format!("Failed to read blob: {}", e)))?;
        crate::log_info(&format!("FGTW: Downloaded blob ({} bytes)", bytes.len()));
        Ok(Some(bytes.to_vec()))
    } else if status == reqwest::StatusCode::NOT_FOUND {
        Ok(None)
    } else {
        let body = response.text().unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}

/// Delete a blob from FGTW storage
///
/// # Arguments
/// * `storage_key` - Base64url-encoded 32-byte key (43 chars)
/// * `device_keypair` - Ed25519 keypair for signing (must match stored auth)
pub async fn delete_blob(storage_key: &str, device_keypair: &Keypair) -> Result<(), BlobError> {
    use ed25519_dalek::Signer;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    // Decode storage key to bytes for signing
    let key_bytes = URL_SAFE_NO_PAD
        .decode(storage_key.as_bytes())
        .map_err(|e| BlobError::Network(format!("Invalid storage key: {}", e)))?;

    // Sign the storage key bytes
    let signature = device_keypair.secret.sign(&key_bytes);

    // Encode headers
    let pubkey_b64 = URL_SAFE_NO_PAD.encode(device_keypair.public.as_bytes());
    let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    let response = client
        .delete(&format!("{}/blob/{}", FGTW_URL, storage_key))
        .header("X-Device-Pubkey", pubkey_b64)
        .header("X-Signature", signature_b64)
        .send()
        .await
        .map_err(|e| BlobError::Network(format!("DELETE request failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        crate::log_info("FGTW: Deleted blob");
        Ok(())
    } else if status == reqwest::StatusCode::NOT_FOUND {
        // Treat not found as success for delete
        Ok(())
    } else if status == reqwest::StatusCode::FORBIDDEN {
        let body = response.text().await.unwrap_or_default();
        Err(BlobError::Unauthorized(body))
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}
