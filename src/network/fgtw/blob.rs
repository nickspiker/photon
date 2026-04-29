use super::fingerprint::Keypair;
use ed25519_dalek::Signer;
use vsf::VsfType;

const FGTW_URL: &str = "https://fgtw.org";

// ============================================================================
// Blob Storage API (VSF section-based) ============================================================================

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

/// Build a signed VSF with ke in header and given section
fn build_signed_blob_vsf(
    keypair: &Keypair,
    section_name: &str,
    fields: Vec<(String, VsfType)>,
) -> Result<Vec<u8>, BlobError> {
    let unsigned_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only(VsfType::ke(keypair.public.as_bytes().to_vec()))
        .add_section(section_name, fields)
        .build()
        .map_err(|e| BlobError::Network(format!("Build VSF: {}", e)))?;

    let hash_bytes = vsf::verification::compute_provenance_hash(&unsigned_bytes)
        .map_err(|e| BlobError::Network(format!("Compute hash: {}", e)))?;

    let signature = keypair.sign(&hash_bytes);

    let mut signed_bytes = unsigned_bytes;
    vsf::verification::fill_provenance_hash(&mut signed_bytes, &hash_bytes)
        .map_err(|e| BlobError::Network(format!("Fill hash: {}", e)))?;
    vsf::verification::fill_signature(&mut signed_bytes, &signature.to_bytes())
        .map_err(|e| BlobError::Network(format!("Fill signature: {}", e)))?;

    Ok(signed_bytes)
}

/// Upload a blob to FGTW storage
///
/// Sends POST / with VSF section "blob_put" containing:
/// - key (d): base64url storage key
/// - signature (ge): Ed25519 signature over key bytes
/// - timestamp (e): Eagle Time oscillations
/// - handle_proof (hP): 32-byte handle proof
/// - data (v'e'): encrypted blob data
pub async fn put_blob(
    storage_key: &str,
    data: &[u8],
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
) -> Result<(), BlobError> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    // Sign the storage key bytes
    let key_bytes = URL_SAFE_NO_PAD
        .decode(storage_key.as_bytes())
        .map_err(|e| BlobError::Network(format!("Invalid storage key: {}", e)))?;
    let key_signature = device_keypair.secret.sign(&key_bytes);

    let vsf_bytes = build_signed_blob_vsf(
        device_keypair,
        "blob_put",
        vec![
            ("key".to_string(), VsfType::d(storage_key.to_string())),
            ("signature".to_string(), VsfType::ge(key_signature.to_bytes().to_vec())),
            ("timestamp".to_string(), VsfType::e(vsf::types::EtType::e6(vsf::eagle_time_oscillations()))),
            ("handle_proof".to_string(), VsfType::hP(handle_proof.to_vec())),
            ("data".to_string(), VsfType::v(b'e', data.to_vec())),
        ],
    )?;

    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .await
        .map_err(|e| BlobError::Network(format!("PUT request failed: {}", e)))?;

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

/// Download a blob from FGTW storage
///
/// Sends POST / with VSF section "blob_get" containing:
/// - key (d): base64url storage key
pub async fn get_blob(storage_key: &str) -> Result<Option<Vec<u8>>, BlobError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    // blob_get doesn't need signing — just a minimal VSF with the key
    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section(
            "blob_get",
            vec![("key".to_string(), VsfType::d(storage_key.to_string()))],
        )
        .build()
        .map_err(|e| BlobError::Network(format!("Build VSF: {}", e)))?;

    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .await
        .map_err(|e| BlobError::Network(format!("GET request failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        let bytes = response
            .bytes()
            .await
            .map_err(|e| BlobError::Network(format!("Failed to read blob: {}", e)))?;

        // Parse VSF response to extract blob data from "blob_data" section
        use vsf::file_format::{VsfHeader, VsfSection};
        let (_, header_end) = VsfHeader::decode(&bytes)
            .map_err(|e| BlobError::Network(format!("Parse response header: {}", e)))?;

        let mut ptr = header_end;
        let section = VsfSection::parse(&bytes, &mut ptr)
            .map_err(|e| BlobError::Network(format!("Parse blob_data section: {}", e)))?;

        let blob_data = section
            .get_field("data")
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::v(_, data) => Some(data.clone()),
                _ => None,
            });

        match blob_data {
            Some(data) => {
                crate::log(&format!("FGTW: Downloaded blob ({} bytes)", data.len()));
                Ok(Some(data))
            }
            None => Ok(None),
        }
    } else if status == reqwest::StatusCode::NOT_FOUND {
        Ok(None)
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}

/// Upload a blob to FGTW storage (blocking version)
pub fn put_blob_blocking(
    storage_key: &str,
    data: &[u8],
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
) -> Result<(), BlobError> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    #[cfg(feature = "development")]
    crate::log("Cloud: put_blob_blocking: creating HTTP client...");

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    #[cfg(feature = "development")]
    crate::log("Cloud: put_blob_blocking: signing request...");

    let key_bytes = URL_SAFE_NO_PAD
        .decode(storage_key.as_bytes())
        .map_err(|e| BlobError::Network(format!("Invalid storage key: {}", e)))?;
    let key_signature = device_keypair.secret.sign(&key_bytes);

    let vsf_bytes = build_signed_blob_vsf(
        device_keypair,
        "blob_put",
        vec![
            ("key".to_string(), VsfType::d(storage_key.to_string())),
            ("signature".to_string(), VsfType::ge(key_signature.to_bytes().to_vec())),
            ("timestamp".to_string(), VsfType::e(vsf::types::EtType::e6(vsf::eagle_time_oscillations()))),
            ("handle_proof".to_string(), VsfType::hP(handle_proof.to_vec())),
            ("data".to_string(), VsfType::v(b'e', data.to_vec())),
        ],
    )?;

    #[cfg(feature = "development")]
    crate::log(&format!("Cloud: put_blob_blocking: sending blob_put VSF..."));

    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| BlobError::Network(format!("PUT request failed: {}", e)))?;

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

/// Download a blob from FGTW storage (blocking version)
pub fn get_blob_blocking(storage_key: &str) -> Result<Option<Vec<u8>>, BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section(
            "blob_get",
            vec![("key".to_string(), VsfType::d(storage_key.to_string()))],
        )
        .build()
        .map_err(|e| BlobError::Network(format!("Build VSF: {}", e)))?;

    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| BlobError::Network(format!("GET request failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        let bytes = response
            .bytes()
            .map_err(|e| BlobError::Network(format!("Failed to read blob: {}", e)))?;

        use vsf::file_format::{VsfHeader, VsfSection};
        let (_, header_end) = VsfHeader::decode(&bytes)
            .map_err(|e| BlobError::Network(format!("Parse response header: {}", e)))?;

        let mut ptr = header_end;
        let section = VsfSection::parse(&bytes, &mut ptr)
            .map_err(|e| BlobError::Network(format!("Parse blob_data section: {}", e)))?;

        let blob_data = section
            .get_field("data")
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::v(_, data) => Some(data.clone()),
                _ => None,
            });

        match blob_data {
            Some(data) => {
                crate::log(&format!("FGTW: Downloaded blob ({} bytes)", data.len()));
                Ok(Some(data))
            }
            None => Ok(None),
        }
    } else if status == reqwest::StatusCode::NOT_FOUND {
        Ok(None)
    } else {
        let body = response.text().unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}

/// Delete a blob from FGTW storage
///
/// Sends POST / with VSF section "blob_delete" containing:
/// - key (d): base64url storage key
/// - signature (ge): Ed25519 signature over key bytes
pub async fn delete_blob(storage_key: &str, device_keypair: &Keypair) -> Result<(), BlobError> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    let key_bytes = URL_SAFE_NO_PAD
        .decode(storage_key.as_bytes())
        .map_err(|e| BlobError::Network(format!("Invalid storage key: {}", e)))?;
    let key_signature = device_keypair.secret.sign(&key_bytes);

    let vsf_bytes = build_signed_blob_vsf(
        device_keypair,
        "blob_delete",
        vec![
            ("key".to_string(), VsfType::d(storage_key.to_string())),
            ("signature".to_string(), VsfType::ge(key_signature.to_bytes().to_vec())),
        ],
    )?;

    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .await
        .map_err(|e| BlobError::Network(format!("DELETE request failed: {}", e)))?;

    let status = response.status();
    if status.is_success() {
        crate::log("FGTW: Deleted blob");
        Ok(())
    } else if status == reqwest::StatusCode::NOT_FOUND {
        Ok(())
    } else if status == reqwest::StatusCode::FORBIDDEN {
        let body = response.text().await.unwrap_or_default();
        Err(BlobError::Unauthorized(body))
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(BlobError::ServerError(format!("{}: {}", status, body)))
    }
}
