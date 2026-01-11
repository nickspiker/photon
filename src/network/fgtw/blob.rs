use super::fingerprint::Keypair;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};

const FGTW_URL: &str = "https://fgtw.org";

/// Schema for blob_data section returned by FGTW
fn blob_data_schema() -> SectionSchema {
    SectionSchema::new("blob_data")
        .field("data", TypeConstraint::Any)  // v type with encryption encoding
        .field("timestamp", TypeConstraint::AnyEagleTime)  // Optional server timestamp
}

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

/// Upload a blob to FGTW storage via conduit
///
/// # Arguments
/// * `storage_key` - Base64url-encoded 32-byte key (43 chars)
/// * `data` - Raw bytes to store (already encrypted by caller)
/// * `device_keypair` - Ed25519 keypair for signing
/// * `handle_proof` - 32-byte handle proof (proves registered user)
pub async fn put_blob(
    storage_key: &str,
    data: &[u8],
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
) -> Result<(), BlobError> {
    use ed25519_dalek::Signer;
    use vsf::VsfType;

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

    // Build VSF message for blob_put conduit operation
    let blob_put_fields = vec![
        ("key".to_string(), VsfType::d(storage_key.to_string())),
        ("device_pubkey".to_string(), VsfType::ke(device_keypair.public.as_bytes().to_vec())),
        ("signature".to_string(), VsfType::ge(signature.to_bytes().to_vec())),
        ("timestamp".to_string(), VsfType::e(vsf::EtType::f6(timestamp))),
        ("handle_proof".to_string(), VsfType::hP(handle_proof.to_vec())),
        ("data".to_string(), VsfType::v(b'e', data.to_vec())),
    ];

    let blob_put_request = vsf::vsf_builder::VsfBuilder::new()
        .creation_time_nanos(timestamp)
        .signed_only(VsfType::ke(device_keypair.public.as_bytes().to_vec()))
        .add_section("blob_put", blob_put_fields)
        .build()
        .map_err(|e| BlobError::Network(format!("Build blob_put request: {}", e)))?;

    let response = client
        .post(&format!("{}/conduit", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(blob_put_request)
        .send()
        .await
        .map_err(|e| BlobError::Network(format!("POST conduit failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        crate::log(&format!("FGTW: Uploaded blob ({} bytes)", data.len()));
        Ok(())
    } else if status == reqwest::StatusCode::FORBIDDEN {
        let body = response.text().await.unwrap_or_default();
        Err(BlobError::Unauthorized(body))
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}

/// Download a blob from FGTW storage via conduit (unauthenticated read)
///
/// # Arguments
/// * `storage_key` - Base64url-encoded 32-byte key (43 chars)
///
/// # Returns
/// * `Ok(Some(bytes))` - Blob data
/// * `Ok(None)` - Blob not found (404)
/// * `Err(...)` - Other error
pub async fn get_blob(storage_key: &str) -> Result<Option<Vec<u8>>, BlobError> {
    use vsf::VsfType;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    // Build VSF message for blob_get conduit operation
    let blob_get_fields = vec![
        ("key".to_string(), VsfType::d(storage_key.to_string())),
    ];

    let blob_get_request = vsf::vsf_builder::VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .provenance_only()
        .add_section("blob_get", blob_get_fields)
        .build()
        .map_err(|e| BlobError::Network(format!("Build blob_get request: {}", e)))?;

    #[cfg(feature = "development")]
    crate::log(&crate::network::inspect::vsf_inspect(
        &blob_get_request,
        "FGTW",
        "TX",
        &format!("conduit/blob_get {}", &storage_key[..8.min(storage_key.len())]),
    ));

    let response = client
        .post(&format!("{}/conduit", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(blob_get_request)
        .send()
        .await
        .map_err(|e| BlobError::Network(format!("POST conduit failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        let bytes = response
            .bytes()
            .await
            .map_err(|e| BlobError::Network(format!("Failed to read response: {}", e)))?;

        #[cfg(feature = "development")]
        crate::log(&crate::network::inspect::vsf_inspect(
            &bytes,
            "FGTW",
            "RX",
            &format!("conduit/blob_get {}", &storage_key[..8.min(storage_key.len())]),
        ));

        // Parse VSF response using SectionBuilder
        use vsf::file_format::VsfHeader;
        use vsf::VsfType;

        if bytes.len() < 4 || &bytes[0..3] != "RÅ".as_bytes() || bytes[3] != b'<' {
            return Err(BlobError::ServerError("Invalid VSF response".to_string()));
        }

        let (_, header_end) = VsfHeader::decode(&bytes)
            .map_err(|e| BlobError::ServerError(format!("Failed to parse VSF header: {}", e)))?;

        let section_bytes = &bytes[header_end..];

        let schema = blob_data_schema();
        let builder = SectionBuilder::parse(schema, section_bytes)
            .map_err(|e| BlobError::ServerError(format!("Parse blob_data: {}", e)))?;

        // Extract data field
        let data_values = builder.get("data")
            .map_err(|e| BlobError::ServerError(format!("No data field: {}", e)))?;

        let blob_bytes = match data_values.first() {
            Some(VsfType::v(_, bytes)) => bytes.clone(),
            _ => return Err(BlobError::ServerError("Invalid data field type".to_string())),
        };

        // Optional: log timestamp if present
        #[cfg(feature = "development")]
        if let Ok(timestamp_values) = builder.get("timestamp") {
            if let Some(VsfType::e(et)) = timestamp_values.first() {
                crate::log(&format!("FGTW: Blob timestamp: {:?}", et));
            }
        }

        crate::log(&format!("FGTW: Downloaded blob ({} bytes)", blob_bytes.len()));
        Ok(Some(blob_bytes))
    } else if status == reqwest::StatusCode::NOT_FOUND {
        Ok(None)
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}

/// Upload a blob to FGTW storage via conduit (blocking version)
///
/// Same as put_blob but uses blocking HTTP client for sync contexts
pub fn put_blob_blocking(
    storage_key: &str,
    data: &[u8],
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
) -> Result<(), BlobError> {
    use ed25519_dalek::Signer;
    use vsf::VsfType;

    #[cfg(feature = "development")]
    crate::log("Cloud: put_blob_blocking: creating HTTP client...");

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    #[cfg(feature = "development")]
    crate::log("Cloud: put_blob_blocking: signing request...");

    // Decode storage key to bytes for signing
    let key_bytes = URL_SAFE_NO_PAD
        .decode(storage_key.as_bytes())
        .map_err(|e| BlobError::Network(format!("Invalid storage key: {}", e)))?;

    // Sign the storage key bytes
    let signature = device_keypair.secret.sign(&key_bytes);

    // Current Eagle Time for replay protection
    let timestamp = vsf::eagle_time_nanos();

    // Build VSF message for blob_put conduit operation
    let blob_put_fields = vec![
        ("key".to_string(), VsfType::d(storage_key.to_string())),
        ("device_pubkey".to_string(), VsfType::ke(device_keypair.public.as_bytes().to_vec())),
        ("signature".to_string(), VsfType::ge(signature.to_bytes().to_vec())),
        ("timestamp".to_string(), VsfType::e(vsf::EtType::f6(timestamp))),
        ("handle_proof".to_string(), VsfType::hP(handle_proof.to_vec())),
        ("data".to_string(), VsfType::v(b'e', data.to_vec())),
    ];

    let blob_put_request = vsf::vsf_builder::VsfBuilder::new()
        .creation_time_nanos(timestamp)
        .signed_only(VsfType::ke(device_keypair.public.as_bytes().to_vec()))
        .add_section("blob_put", blob_put_fields)
        .build()
        .map_err(|e| BlobError::Network(format!("Build blob_put request: {}", e)))?;

    #[cfg(feature = "development")]
    crate::log(&format!("Cloud: put_blob_blocking: sending POST to conduit/blob_put (key: {}...)", &storage_key[..8.min(storage_key.len())]));

    let response = client
        .post(&format!("{}/conduit", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(blob_put_request)
        .send()
        .map_err(|e| BlobError::Network(format!("POST conduit failed: {}", e)))?;

    #[cfg(feature = "development")]
    crate::log(&format!("Cloud: put_blob_blocking: response status {}", response.status()));

    let status = response.status();
    if status.is_success() {
        crate::log(&format!("FGTW: Uploaded blob ({} bytes)", data.len()));
        Ok(())
    } else if status == reqwest::StatusCode::FORBIDDEN {
        let body = response.text().unwrap_or_default();
        Err(BlobError::Unauthorized(body))
    } else {
        let body = response.text().unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}

/// Download a blob from FGTW storage via conduit (blocking version)
///
/// Same as get_blob but uses blocking HTTP client for sync contexts
pub fn get_blob_blocking(storage_key: &str) -> Result<Option<Vec<u8>>, BlobError> {
    use vsf::VsfType;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    // Build VSF message for blob_get conduit operation
    let blob_get_fields = vec![
        ("key".to_string(), VsfType::d(storage_key.to_string())),
    ];

    let blob_get_request = vsf::vsf_builder::VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .provenance_only()
        .add_section("blob_get", blob_get_fields)
        .build()
        .map_err(|e| BlobError::Network(format!("Build blob_get request: {}", e)))?;

    #[cfg(feature = "development")]
    crate::log(&crate::network::inspect::vsf_inspect(
        &blob_get_request,
        "FGTW",
        "TX",
        &format!("conduit/blob_get {}", &storage_key[..8.min(storage_key.len())]),
    ));

    let response = client
        .post(&format!("{}/conduit", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(blob_get_request)
        .send()
        .map_err(|e| BlobError::Network(format!("POST conduit failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        let bytes = response
            .bytes()
            .map_err(|e| BlobError::Network(format!("Failed to read response: {}", e)))?;

        #[cfg(feature = "development")]
        crate::log(&crate::network::inspect::vsf_inspect(
            &bytes,
            "FGTW",
            "RX",
            &format!("conduit/blob_get {}", &storage_key[..8.min(storage_key.len())]),
        ));

        // Parse VSF response using SectionBuilder
        use vsf::file_format::VsfHeader;
        use vsf::VsfType;

        if bytes.len() < 4 || &bytes[0..3] != "RÅ".as_bytes() || bytes[3] != b'<' {
            return Err(BlobError::ServerError("Invalid VSF response".to_string()));
        }

        let (_, header_end) = VsfHeader::decode(&bytes)
            .map_err(|e| BlobError::ServerError(format!("Failed to parse VSF header: {}", e)))?;

        let section_bytes = &bytes[header_end..];

        let schema = blob_data_schema();
        let builder = SectionBuilder::parse(schema, section_bytes)
            .map_err(|e| BlobError::ServerError(format!("Parse blob_data: {}", e)))?;

        // Extract data field
        let data_values = builder.get("data")
            .map_err(|e| BlobError::ServerError(format!("No data field: {}", e)))?;

        let blob_bytes = match data_values.first() {
            Some(VsfType::v(_, bytes)) => bytes.clone(),
            _ => return Err(BlobError::ServerError("Invalid data field type".to_string())),
        };

        // Optional: log timestamp if present
        #[cfg(feature = "development")]
        if let Ok(timestamp_values) = builder.get("timestamp") {
            if let Some(VsfType::e(et)) = timestamp_values.first() {
                crate::log(&format!("FGTW: Blob timestamp: {:?}", et));
            }
        }

        crate::log(&format!("FGTW: Downloaded blob ({} bytes)", blob_bytes.len()));
        Ok(Some(blob_bytes))
    } else if status == reqwest::StatusCode::NOT_FOUND {
        Ok(None)
    } else {
        let body = response.text().unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}

/// Delete a blob from FGTW storage via conduit
///
/// # Arguments
/// * `storage_key` - Base64url-encoded 32-byte key (43 chars)
/// * `device_keypair` - Ed25519 keypair for signing (must match stored auth)
pub async fn delete_blob(storage_key: &str, device_keypair: &Keypair) -> Result<(), BlobError> {
    use ed25519_dalek::Signer;
    use vsf::VsfType;

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

    // Build VSF message for blob_delete conduit operation
    let blob_delete_fields = vec![
        ("key".to_string(), VsfType::d(storage_key.to_string())),
        ("device_pubkey".to_string(), VsfType::ke(device_keypair.public.as_bytes().to_vec())),
        ("signature".to_string(), VsfType::ge(signature.to_bytes().to_vec())),
    ];

    let blob_delete_request = vsf::vsf_builder::VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .provenance_only()
        .add_section("blob_delete", blob_delete_fields)
        .build()
        .map_err(|e| BlobError::Network(format!("Build blob_delete request: {}", e)))?;

    let response = client
        .post(&format!("{}/conduit", FGTW_URL))
        .header("Content-Type", "application/octet-stream")
        .body(blob_delete_request)
        .send()
        .await
        .map_err(|e| BlobError::Network(format!("POST conduit failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        crate::log("FGTW: Deleted blob");
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
