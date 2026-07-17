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
            (
                "signature".to_string(),
                VsfType::ge(key_signature.to_bytes().to_vec()),
            ),
            (
                "timestamp".to_string(),
                VsfType::e(vsf::types::EtType::e6(vsf::eagle_time_oscillations())),
            ),
            (
                "handle_proof".to_string(),
                VsfType::hP(handle_proof.to_vec()),
            ),
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
    let body = response.bytes().await.unwrap_or_default();
    if let Some((reason, detail)) = fgtw::client::error_frame(&body) {
        // slot_owned / replay are the ownership rejections the worker sends for blob_put; surface them as Unauthorized, the rest as ServerError.
        return Err(match reason.as_str() {
            "slot_owned" | "replay" => BlobError::Unauthorized(format!("{reason}: {detail}")),
            _ => BlobError::ServerError(format!("{reason}: {detail}")),
        });
    }
    if !status.is_success() {
        return Err(BlobError::ServerError(format!("transport {}", status)));
    }
    crate::logf!("FGTW: Uploaded blob ({} bytes)", data.len());
    Ok(())
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
    let bytes = response
        .bytes()
        .await
        .map_err(|e| BlobError::Network(format!("Failed to read blob: {}", e)))?;

    if fgtw::client::is_error(&bytes, "not_found") {
        return Ok(None);
    }
    if let Some((reason, detail)) = fgtw::client::error_frame(&bytes) {
        return Err(BlobError::ServerError(format!("{reason}: {detail}")));
    }
    if !status.is_success() {
        return Err(BlobError::ServerError(format!("transport {}", status)));
    }

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
            crate::logf!("FGTW: Downloaded blob ({} bytes)", data.len());
            Ok(Some(data))
        }
        None => Ok(None),
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
            (
                "signature".to_string(),
                VsfType::ge(key_signature.to_bytes().to_vec()),
            ),
            (
                "timestamp".to_string(),
                VsfType::e(vsf::types::EtType::e6(vsf::eagle_time_oscillations())),
            ),
            (
                "handle_proof".to_string(),
                VsfType::hP(handle_proof.to_vec()),
            ),
            ("data".to_string(), VsfType::v(b'e', data.to_vec())),
        ],
    )?;

    #[cfg(feature = "development")]
    crate::logf!("Cloud: put_blob_blocking: sending blob_put VSF...");

    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| BlobError::Network(format!("PUT request failed: {}", e)))?;

    #[cfg(feature = "development")]
    crate::logf!("Cloud: put_blob_blocking: response status {}", response.status());

    let status = response.status();
    let body = response.bytes().unwrap_or_default();
    if let Some((reason, detail)) = fgtw::client::error_frame(&body) {
        return Err(match reason.as_str() {
            "slot_owned" | "replay" => BlobError::Unauthorized(format!("{reason}: {detail}")),
            _ => BlobError::ServerError(format!("{reason}: {detail}")),
        });
    }
    if !status.is_success() {
        return Err(BlobError::ServerError(format!("transport {}", status)));
    }
    crate::logf!("FGTW: Uploaded blob ({} bytes)", data.len());
    Ok(())
}

/// Submits the on-device diagnostic log to FGTW (the "press log → it lands where the dev can pull it" flow).
/// Sends POST / with VSF section "log_put": the log bytes + optional note + handle_proof + timestamp, the whole frame canonically signed by the device key (worker verifies via `read_verified`, so the log bytes are authenticated, not just a detached key). Storing on FGTW instead of on-device is deliberate for now — an outbound HTTPS POST is NAT-immune, so it works exactly where the P2P path is failing, and it needs no USB pull. The worker keys each submission by timestamp, so this ADDS to a device's log history rather than overwriting.
pub fn put_log_blocking(
    log_bytes: &[u8],
    note: &str,
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
    identity_seed: &[u8; 32],
) -> Result<(), BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    // Seal the log (and note) on the client BEFORE it leaves the device — ChaCha20-Poly1305 under a key derived from the identity seed, so no plaintext ever hits the wire and the R2 blob is opaque to anyone who can't re-derive the key from the handle. The `v'e'` encoding byte marks the value encrypted (VSF-proper); the worker stores the ciphertext verbatim.
    let key = crate::log_encryption_key(identity_seed);
    let sealed_log =
        crate::storage::encrypt_bytes(log_bytes, &key).map_err(|e| BlobError::Network(format!("Log encrypt: {e}")))?;
    // The retrieval tag is what indexes this log on the server: spaghettify(domain ‖ seed), a one-way capability. The worker stores under it; a puller who knows the seed re-derives it to find the log. The seed itself never leaves the device.
    let tag = crate::log_retrieval_tag(identity_seed);
    // Anti-spam gate: an explicit device-key signature over the tag (the worker verifies this, mirroring blob_put's signature-over-key — build_signed_blob_vsf's header signature is provenance-only and not read_verified-checkable).
    let tag_signature = device_keypair.secret.sign(&tag);

    let mut fields = vec![
        (
            "timestamp".to_string(),
            VsfType::e(vsf::types::EtType::e6(vsf::eagle_time_oscillations())),
        ),
        ("handle_proof".to_string(), VsfType::hP(handle_proof.to_vec())),
        ("tag".to_string(), VsfType::v(b'r', tag.to_vec())),
        ("signature".to_string(), VsfType::ge(tag_signature.to_bytes().to_vec())),
        ("data".to_string(), VsfType::v(b'e', sealed_log)),
    ];
    // The optional note rides only when the user typed one — a blank field is simply absent (the worker treats a missing note as "").
    // Sealed under the same key as the log — the note can carry sensitive context too, so it never hits the wire in the clear either.
    if !note.is_empty() {
        let sealed_note =
            crate::storage::encrypt_bytes(note.as_bytes(), &key).map_err(|e| BlobError::Network(format!("Note encrypt: {e}")))?;
        fields.push(("note".to_string(), VsfType::v(b'e', sealed_note)));
    }

    let vsf_bytes = build_signed_blob_vsf(device_keypair, "log_put", fields)?;

    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| BlobError::Network(format!("log_put request failed: {}", e)))?;

    let status = response.status();
    let body = response.bytes().unwrap_or_default();
    if let Some((reason, detail)) = fgtw::client::error_frame(&body) {
        return Err(BlobError::ServerError(format!("{reason}: {detail}")));
    }
    if !status.is_success() {
        return Err(BlobError::ServerError(format!("transport {}", status)));
    }
    crate::logf!("FGTW: submitted diagnostic log ({} bytes)", log_bytes.len());
    Ok(())
}

/// List the submitted logs for a retrieval tag (the pull side of the capability).
/// Sends `log_list { tag }`; the worker enumerates its `photon-logs/<tag>/` prefix and returns the object keys. Unsigned — presenting the tag (which only the seed-holder can derive) IS the capability, so no device signature is required. `tag` = [`crate::log_retrieval_tag`] of the target identity seed.
/// `log_delete` — the LastRites sweep for submitted diagnostic logs: delete everything under this identity's retrieval tag (docs/lifecycle.md — the log keyspace is deliberately unlinkable to the handle_proof, so the fleet purge can't reach it; the departing client must). Tag = the capability, same as list/get. Idempotent.
pub fn log_delete_blocking(tag: &[u8; 32]) -> Result<(), BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;
    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section("log_delete", vec![("tag".to_string(), VsfType::v(b'r', tag.to_vec()))])
        .build()
        .map_err(|e| BlobError::Network(format!("Build VSF: {}", e)))?;
    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| BlobError::Network(format!("log_delete request failed: {}", e)))?;
    let bytes = response.bytes().unwrap_or_default();
    if let Some((reason, detail)) = fgtw::client::error_frame(&bytes) {
        return Err(BlobError::ServerError(format!("{reason}: {detail}")));
    }
    Ok(())
}

pub fn log_list_blocking(tag: &[u8; 32]) -> Result<Vec<String>, BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section("log_list", vec![("tag".to_string(), VsfType::v(b'r', tag.to_vec()))])
        .build()
        .map_err(|e| BlobError::Network(format!("Build VSF: {}", e)))?;

    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| BlobError::Network(format!("log_list request failed: {}", e)))?;
    let bytes = response.bytes().unwrap_or_default();
    if let Some((reason, detail)) = fgtw::client::error_frame(&bytes) {
        return Err(BlobError::ServerError(format!("{reason}: {detail}")));
    }
    // Schema-validated parse (vsf trust gate): the worker returns the keys newline-joined in one `d` field.
    let schema = vsf::schema::SectionSchema::new("log_list_ack")
        .field("keys", vsf::schema::TypeConstraint::DictKey);
    let section = vsf::schema::SectionBuilder::parse_document(schema, &bytes, None)
        .map_err(|e| BlobError::Network(format!("Parse log_list_ack: {}", e)))?;
    let joined = section.get_value::<String>("keys").unwrap_or_default();
    Ok(joined.lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect())
}

/// Fetch one submitted log blob (still ChaCha20-Poly1305 ciphertext) by its full storage key.
/// Sends `log_get { key }`; the worker returns the stored bytes. The key contains the tag prefix, so possessing it is the capability. Caller decrypts with [`crate::log_encryption_key`] of the same seed.
pub fn log_get_blocking(key: &str) -> Result<Vec<u8>, BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section("log_get", vec![("key".to_string(), VsfType::d(key.to_string()))])
        .build()
        .map_err(|e| BlobError::Network(format!("Build VSF: {}", e)))?;

    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| BlobError::Network(format!("log_get request failed: {}", e)))?;
    let bytes = response.bytes().unwrap_or_default();
    if let Some((reason, detail)) = fgtw::client::error_frame(&bytes) {
        return Err(BlobError::ServerError(format!("{reason}: {detail}")));
    }
    // Schema-validated parse (vsf trust gate): the response is `log_data { data: v'e' ciphertext }`.
    let schema = vsf::schema::SectionSchema::new("log_data")
        .field("data", vsf::schema::TypeConstraint::Wrapped(b'e'));
    let section = vsf::schema::SectionBuilder::parse_document(schema, &bytes, None)
        .map_err(|e| BlobError::Network(format!("Parse log_data: {}", e)))?;
    section
        .get_value::<Vec<u8>>("data")
        .map_err(|e| BlobError::ServerError(format!("log_get data: {}", e)))
}

/// One drained fleet-inbox event (docs/fleet-inbox.md). `kind` is the ASCII event tag ("bind_attempt"); `device` the device pubkey it concerned; `attempted_by` the handle_proof of whoever triggered it (the fleet a bind was attempted into) — rendered as a contact name if known, opaque otherwise; `t_osc` its eagle-time.
#[derive(Debug, Clone)]
pub struct FleetInboxEvent {
    pub kind: String,
    pub device: [u8; 32],
    pub attempted_by: [u8; 32],
    pub t_osc: i64,
}

/// Drain (and consume) this identity's pending fleet-inbox events.
/// Sends `inbox_drain { hp }` signed by the device key; the worker verifies the device is a CURRENT member of that fleet before returning + deleting the events (so only our own devices read/clear our alerts). The response `events` field is concatenated complete VSF docs — each self-delimiting via its header file_length — which we split and parse here.
pub fn inbox_drain_blocking(
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
) -> Result<Vec<FleetInboxEvent>, BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    // Canonical whole-file signing (ge over BLAKE3(file, ge zeroed)) — the scheme the worker's verify_file_signature_webcrypto checks (build_signed_blob_vsf's header is provenance-only, which that verify rejects; see log_put's detached-signature note).
    let unsigned = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only(VsfType::ke(device_keypair.public.as_bytes().to_vec()))
        .add_section("inbox_drain", vec![("hp".to_string(), VsfType::hP(handle_proof.to_vec()))])
        .build()
        .map_err(|e| BlobError::Network(format!("Build VSF: {}", e)))?;
    let vsf_bytes = vsf::verification::sign_file(unsigned, device_keypair.secret.as_bytes())
        .map_err(|e| BlobError::Network(format!("Sign inbox_drain: {}", e)))?;

    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| BlobError::Network(format!("inbox_drain request failed: {}", e)))?;
    let bytes = response.bytes().unwrap_or_default();
    if let Some((reason, detail)) = fgtw::client::error_frame(&bytes) {
        // not_member on a fresh device that hasn't folded yet is benign — treat as "nothing to drain".
        if reason == "not_member" {
            return Ok(Vec::new());
        }
        return Err(BlobError::ServerError(format!("{reason}: {detail}")));
    }
    // Response is `inbox_drain_ack { events: v'r' concat-of-73-byte-records }` — schema-validated (vsf trust gate).
    let schema = vsf::schema::SectionSchema::new("inbox_drain_ack")
        .field("events", vsf::schema::TypeConstraint::Wrapped(b'r'));
    let section = vsf::schema::SectionBuilder::parse_document(schema, &bytes, None)
        .map_err(|e| BlobError::Network(format!("Parse inbox_drain_ack: {}", e)))?;
    let blob = section.get_value::<Vec<u8>>("events").unwrap_or_default();
    Ok(parse_inbox_events(&blob))
}

/// 73-byte inbox record: `[kind:u8][dev:32][by:32][t_osc:i64-be:8]` (see the worker's write_inbox_event). Kind 0 = bind_attempt.
const INBOX_REC_LEN: usize = 73;

/// Split a concatenated stream of fixed 73-byte inbox records into [`FleetInboxEvent`]s.
/// Raw fixed-layout (not VSF) so this stays plain byte slicing — no hand-rolled VSF read at a trust boundary (the outer drain response was already schema-validated). A trailing partial record is ignored.
fn parse_inbox_events(blob: &[u8]) -> Vec<FleetInboxEvent> {
    let mut out = Vec::new();
    for rec in blob.chunks_exact(INBOX_REC_LEN) {
        let kind = match rec[0] {
            0 => "bind_attempt".to_string(),
            other => format!("kind_{other}"),
        };
        let mut device = [0u8; 32];
        device.copy_from_slice(&rec[1..33]);
        let mut attempted_by = [0u8; 32];
        attempted_by.copy_from_slice(&rec[33..65]);
        let t_osc = i64::from_be_bytes(rec[65..73].try_into().unwrap());
        out.push(FleetInboxEvent { kind, device, attempted_by, t_osc });
    }
    out
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
    let bytes = response
        .bytes()
        .map_err(|e| BlobError::Network(format!("Failed to read blob: {}", e)))?;

    if fgtw::client::is_error(&bytes, "not_found") {
        return Ok(None);
    }
    if let Some((reason, detail)) = fgtw::client::error_frame(&bytes) {
        return Err(BlobError::ServerError(format!("{reason}: {detail}")));
    }
    if !status.is_success() {
        return Err(BlobError::ServerError(format!("transport {}", status)));
    }

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
            crate::logf!("FGTW: Downloaded blob ({} bytes)", data.len());
            Ok(Some(data))
        }
        None => Ok(None),
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
            (
                "signature".to_string(),
                VsfType::ge(key_signature.to_bytes().to_vec()),
            ),
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
    let body = response.bytes().await.unwrap_or_default();
    // not_found → idempotent success (already gone); slot_owned → ownership rejection.
    if fgtw::client::is_error(&body, "not_found") {
        return Ok(());
    }
    if let Some((reason, detail)) = fgtw::client::error_frame(&body) {
        return Err(match reason.as_str() {
            "slot_owned" => BlobError::Unauthorized(format!("{reason}: {detail}")),
            _ => BlobError::ServerError(format!("{reason}: {detail}")),
        });
    }
    if !status.is_success() {
        return Err(BlobError::ServerError(format!("transport {}", status)));
    }
    crate::log("FGTW: Deleted blob");
    Ok(())
}

#[cfg(test)]
mod log_capability_tests {
    use super::*;

    // Network smoke test against the LIVE fgtw.org worker: submit a sealed log, then pull it back by the
    // seed-derived tag and decrypt it. Run explicitly: `cargo test --features development -- --ignored roundtrip`.
    #[test]
    #[ignore]
    fn roundtrip_submit_list_get_decrypt() {
        let seed = [0x42u8; 32];
        let kp = super::super::derive_device_keypair(b"photonlog-smoke-fingerprint");
        let hp = [0x11u8; 32];
        let payload = b"SMOKE photon.log capability roundtrip payload".to_vec();

        put_log_blocking(&payload, "smoke note", &kp, &hp, &seed).expect("submit");

        let tag = crate::log_retrieval_tag(&seed);
        let keys = log_list_blocking(&tag).expect("list");
        assert!(!keys.is_empty(), "no keys listed under the tag after submit");

        let ct = log_get_blocking(&keys[0]).expect("get");
        let plain = crate::storage::decrypt_bytes(&ct, &crate::log_encryption_key(&seed)).expect("decrypt");
        assert_eq!(plain, payload, "decrypted log must match what was submitted");
    }
}
