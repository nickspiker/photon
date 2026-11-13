use super::fingerprint::Keypair;
use ed25519_dalek::Signer;
use vsf::VsfType;

use crate::network::http::SEED_HTTPS as FGTW_URL;

// ============================================================================ Blob Storage API (VSF section-based) ============================================================================

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
        .creation_time_oscillations(crate::network::time_base::now_osc())
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
// The blob store is fgtw.org; ferros does not speak to it, so every call below answers with a network error there, the way an unreachable server would.
#[cfg(target_os = "ferros")]
fn unreachable_from_ferros<T>() -> Result<T, BlobError> {
    Err(BlobError::Network("no fgtw.org from ferros".to_string()))
}

#[cfg(target_os = "ferros")]
pub async fn put_blob(_storage_key: &str, _data: &[u8], _device_keypair: &Keypair, _handle_proof: &[u8; 32]) -> Result<(), BlobError> {
    unreachable_from_ferros()
}

#[cfg(not(target_os = "ferros"))]
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
            ("key".to_string(), VsfType::d(vsf::Ascii::new(storage_key).map_err(|e| BlobError::Network(format!("key: {e}")))?)),
            (
                "signature".to_string(),
                VsfType::ge(key_signature.to_bytes().to_vec()),
            ),
            (
                "timestamp".to_string(),
                VsfType::e(vsf::types::EtType::e6(crate::network::time_base::now_osc())),
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
#[cfg(target_os = "ferros")]
pub async fn get_blob(_storage_key: &str) -> Result<Option<Vec<u8>>, BlobError> {
    unreachable_from_ferros()
}

#[cfg(not(target_os = "ferros"))]
pub async fn get_blob(storage_key: &str) -> Result<Option<Vec<u8>>, BlobError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    // blob_get doesn't need signing — just a minimal VSF with the key
    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(crate::network::time_base::now_osc())
        .add_section(
            "blob_get",
            vec![("key".to_string(), VsfType::d(vsf::Ascii::new(storage_key).map_err(|e| BlobError::Network(format!("key: {e}")))?))],
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
#[cfg(target_os = "ferros")]
pub fn put_blob_blocking(_storage_key: &str, _data: &[u8], _device_keypair: &Keypair, _handle_proof: &[u8; 32]) -> Result<(), BlobError> {
    unreachable_from_ferros()
}

#[cfg(not(target_os = "ferros"))]
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
            ("key".to_string(), VsfType::d(vsf::Ascii::new(storage_key).map_err(|e| BlobError::Network(format!("key: {e}")))?)),
            (
                "signature".to_string(),
                VsfType::ge(key_signature.to_bytes().to_vec()),
            ),
            (
                "timestamp".to_string(),
                VsfType::e(vsf::types::EtType::e6(crate::network::time_base::now_osc())),
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
    crate::logf!(
        "Cloud: put_blob_blocking: response status {}",
        response.status()
    );

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
/// The log and its note are sealed to the maintainer's log key alone ([`crate::crypto::log_seal`], ferros docs/PHOTON.md M6) and filed under this identity's handle proof, so the maintainer finds them by typing the handle and nobody else can open them.
/// Sends `log_submit`, signed by this device for its identity: the worker files only for a current member of that fleet. Storing on FGTW instead of on-device is deliberate for now: an outbound HTTPS POST is NAT-immune, so it works exactly where the P2P path is failing, and it needs no USB pull. Each submission is filed by its own time, so this ADDS to a device's log history rather than overwriting.
#[cfg(target_os = "ferros")]
pub fn put_log_blocking(_log_bytes: &[u8], _note: &str, _device_keypair: &Keypair, _handle_proof: &[u8; 32]) -> Result<(), BlobError> {
    unreachable_from_ferros()
}

#[cfg(not(target_os = "ferros"))]
pub fn put_log_blocking(log_bytes: &[u8], note: &str, device_keypair: &Keypair, handle_proof: &[u8; 32]) -> Result<(), BlobError> {
    let client = reqwest::blocking::Client::builder()
        // 300s: a capped-at-24MB log on a slow cellular uplink (1 Mbps up ≈ 3+ min) must finish, not die mid-body as "error sending request".
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    // Sealed on the client BEFORE it leaves the device, to the maintainer's key: no plaintext on the wire, and the filed blob opens for nobody else, the submitter included.
    let sealed = crate::crypto::log_seal::seal(log_bytes, note, &crate::crypto::log_seal::LOG_KEY_PUBLIC).map_err(|e| BlobError::Network(format!("Log seal: {e}")))?;
    let mut fields = vec![
        ("ephemeral".to_string(), VsfType::kx(sealed.ephemeral.to_vec())),
        ("data".to_string(), VsfType::v(b'e', sealed.log)),
    ];
    // The note rides only when the user typed one; a blank one is simply absent.
    if let Some(note) = sealed.note {
        fields.push(("note".to_string(), VsfType::v(b'e', note)));
    }

    // ONE RETRY ON A WINDOW REFUSAL (2026-09-17, Theresa's "timestamp outside valid window" on every submit): the worker's refusal carries its own clock; adopt it as photon's time base and sign the document again, so its creation time is stamped afresh. The server judging the window is the time source that matters for this frame.
    let mut retried = false;
    loop {
        let body = super::octopus::member_signed(device_keypair, handle_proof, "log_submit", fields.clone()).map_err(BlobError::Network)?;
        let sent_at = std::time::Instant::now();
        let response = client
            .post(FGTW_URL)
            .header("Content-Type", "application/octet-stream")
            .body(body)
            .send()
            .map_err(|e| BlobError::Network(format!("log_submit request failed: {}", e)))?;
        let status = response.status();
        let body = response.bytes().unwrap_or_default();
        if let Some((reason, detail)) = fgtw::client::error_frame(&body) {
            if let (Some(server_now), false) = (crate::network::time_base::server_now_from_detail(&detail), retried) {
                let rtt_osc = (sent_at.elapsed().as_secs_f64() * crate::OSC_PER_SEC as f64) as i64;
                crate::network::time_base::adopt_from_server(server_now, rtt_osc);
                retried = true;
                continue;
            }
            return Err(BlobError::ServerError(format!("{reason}: {detail}")));
        }
        if !status.is_success() {
            return Err(BlobError::ServerError(format!("transport {}", status)));
        }
        crate::logf!("FGTW: submitted diagnostic log ({} bytes){}", log_bytes.len(), if retried { " — on the second try, after the server re-anchored our clock" } else { "" });
        return Ok(());
    }
}

/// List the submitted logs for a retrieval tag (the pull side of the capability).
/// Sends `log_list { tag }`; the worker enumerates its `photon-logs/<tag>/` prefix and returns the object keys. Unsigned — presenting the tag (which only the seed-holder can derive) IS the capability, so no device signature is required. `tag` = [`crate::log_retrieval_tag`] of the target identity seed.
/// `log_delete` — the LastRites sweep for submitted diagnostic logs: delete everything under this identity's retrieval tag (docs/lifecycle.md — the log keyspace is deliberately unlinkable to the handle_proof, so the fleet purge can't reach it; the departing client must). Tag = the capability, same as list/get. Idempotent.
#[cfg(target_os = "ferros")]
pub fn log_delete_blocking(_tag: &[u8; 32]) -> Result<(), BlobError> {
    unreachable_from_ferros()
}

// LEGACY LOG PULL — MIGRATION-EXPIRES: 2027-02-01: a pre-M6 Photon's logs, filed under a seed-derived tag; the maintainer's photonlog reads them until those Photons have updated.
#[cfg(not(target_os = "ferros"))]
pub fn log_delete_blocking(tag: &[u8; 32]) -> Result<(), BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;
    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(crate::network::time_base::now_osc())
        .add_section(
            "log_delete",
            vec![("tag".to_string(), VsfType::v(b'r', tag.to_vec()))],
        )
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

#[cfg(target_os = "ferros")]
pub fn log_list_blocking(_tag: &[u8; 32]) -> Result<Vec<String>, BlobError> {
    unreachable_from_ferros()
}

// LEGACY LOG PULL — MIGRATION-EXPIRES: 2027-02-01: a pre-M6 Photon's logs, filed under a seed-derived tag; the maintainer's photonlog reads them until those Photons have updated.
#[cfg(not(target_os = "ferros"))]
pub fn log_list_blocking(tag: &[u8; 32]) -> Result<Vec<String>, BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(crate::network::time_base::now_osc())
        .add_section(
            "log_list",
            vec![("tag".to_string(), VsfType::v(b'r', tag.to_vec()))],
        )
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
    Ok(joined
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect())
}

/// Fetch one submitted log blob (still ChaCha20-Poly1305 ciphertext) by its full storage key.
/// Sends `log_get { key }`; the worker returns the stored bytes. The key contains the tag prefix, so possessing it is the capability. Caller decrypts with [`crate::log_encryption_key`] of the same seed.
#[cfg(target_os = "ferros")]
pub fn log_get_blocking(_key: &str) -> Result<Vec<u8>, BlobError> {
    unreachable_from_ferros()
}

// LEGACY LOG PULL — MIGRATION-EXPIRES: 2027-02-01: a pre-M6 Photon's logs, filed under a seed-derived tag; the maintainer's photonlog reads them until those Photons have updated.
#[cfg(not(target_os = "ferros"))]
pub fn log_get_blocking(key: &str) -> Result<Vec<u8>, BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(crate::network::time_base::now_osc())
        .add_section(
            "log_get",
            vec![("key".to_string(), VsfType::d(vsf::Ascii::new(key).map_err(|e| BlobError::Network(format!("key: {e}")))?))],
        )
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

/// One log as fgtw.org filed it under a handle proof: the worker's complete VSF document (section `log`), still sealed to the maintainer's log key.
#[derive(Clone, Debug)]
pub struct FiledLog {
    pub device: [u8; 32],
    pub filed_osc: i64,
    pub ephemeral: [u8; 32],
    pub log: Vec<u8>,
    pub note: Option<Vec<u8>>,
}

/// Read a filed log's document; every field is checked by its type marker.
pub fn parse_filed_log(bytes: &[u8]) -> Result<FiledLog, BlobError> {
    let sections = crate::storage::record::verified_sections(bytes, None).map_err(|e| BlobError::Network(format!("filed log: {e}")))?;
    let s = sections.iter().find(|s| s.name == "log").ok_or_else(|| BlobError::Network("filed log: no log section".into()))?;
    let first = |n: &str| s.get_field(n).and_then(|f| f.values.first());
    let key32 = |v: Option<&VsfType>| -> Option<[u8; 32]> {
        match v? {
            VsfType::ke(b) | VsfType::kx(b) if b.len() == 32 => b.as_slice().try_into().ok(),
            _ => None,
        }
    };
    let device = key32(first("device")).ok_or_else(|| BlobError::Network("filed log: no device".into()))?;
    let ephemeral = key32(first("ephemeral")).ok_or_else(|| BlobError::Network("filed log: no ephemeral key".into()))?;
    let filed_osc = s.eagle("filed").ok_or_else(|| BlobError::Network("filed log: no time".into()))?;
    let log = match first("data") {
        Some(VsfType::v(b'e', b)) => b.clone(),
        _ => return Err(BlobError::Network("filed log: no sealed data".into())),
    };
    let note = match first("note") {
        Some(VsfType::v(b'e', b)) => Some(b.clone()),
        _ => None,
    };
    Ok(FiledLog { device, filed_osc, ephemeral, log, note })
}

/// The keys of every log filed under a handle proof (`log_list { hp }`); unsigned, since what it names opens only for the maintainer.
pub fn log_list_by_proof_blocking(handle_proof: &[u8; 32]) -> Result<Vec<String>, BlobError> {
    let mut sec = vsf::file_format::VsfSection::new("log_list");
    sec.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    let bytes = gripe_post(sec, "log_list", None)?;
    let schema = vsf::schema::SectionSchema::new("log_list_ack").field("keys", vsf::schema::TypeConstraint::DictKey);
    let section = vsf::schema::SectionBuilder::parse_document(schema, &bytes, None).map_err(|e| BlobError::Network(format!("Parse log_list_ack: {}", e)))?;
    let joined = section.get_value::<String>("keys").unwrap_or_default();
    Ok(joined.lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect())
}

/// One filed log's document, verbatim (`log_get { key }` on a key under `photon-logs/hp/`).
pub fn log_get_filed_blocking(key: &str) -> Result<Vec<u8>, BlobError> {
    let mut sec = vsf::file_format::VsfSection::new("log_get");
    sec.add_field("key", VsfType::d(vsf::Ascii::new(key).map_err(|e| BlobError::Network(format!("key: {e}")))?));
    gripe_post(sec, "log_get", None)
}

/// One line of the maintainer's mailbox: whose log, when it was filed, its size and its key.
#[derive(Clone, Debug)]
pub struct MailboxEntry {
    pub handle_proof: [u8; 32],
    pub filed_osc: i64,
    pub size: u64,
    pub key: String,
}

/// Every log filed on fgtw.org, who and when (`log_mailbox`), signed by the release key: only the maintainer reads the mailbox.
pub fn log_mailbox_blocking(signing_key: &[u8; 32]) -> Result<Vec<MailboxEntry>, BlobError> {
    let bytes = gripe_post(vsf::file_format::VsfSection::new("log_mailbox"), "log_mailbox", Some(signing_key))?;
    let sections = crate::storage::record::verified_sections(&bytes, None).map_err(|e| BlobError::Network(format!("log_mailbox answer: {e}")))?;
    let mut out = Vec::new();
    for s in sections.iter().filter(|s| s.name == "filed") {
        let first = |n: &str| s.get_field(n).and_then(|f| f.values.first());
        let handle_proof = match first("hp") {
            Some(VsfType::hP(b)) if b.len() == 32 => b.as_slice().try_into().map_err(|_| BlobError::Network("mailbox hp".into()))?,
            _ => return Err(BlobError::Network("log_mailbox: an entry without its person".into())),
        };
        let filed_osc = s.eagle("filed").ok_or_else(|| BlobError::Network("log_mailbox: an entry without its time".into()))?;
        let size = first("size").and_then(|v| v.as_u64()).ok_or_else(|| BlobError::Network("log_mailbox: an entry without its size".into()))?;
        let key = match first("key") {
            Some(VsfType::d(k)) => k.to_string(),
            _ => return Err(BlobError::Network("log_mailbox: an entry without its key".into())),
        };
        out.push(MailboxEntry { handle_proof, filed_osc, size, key });
    }
    Ok(out)
}

/// Delete every log filed under a handle proof once the maintainer holds them (`log_consume { hp }`), signed by the release key.
pub fn log_consume_blocking(signing_key: &[u8; 32], handle_proof: &[u8; 32]) -> Result<(), BlobError> {
    let mut sec = vsf::file_format::VsfSection::new("log_consume");
    sec.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    gripe_post(sec, "log_consume", Some(signing_key)).map(|_| ())
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
#[cfg(target_os = "ferros")]
pub fn inbox_drain_blocking(_device_keypair: &Keypair, _handle_proof: &[u8; 32]) -> Result<Vec<FleetInboxEvent>, BlobError> {
    unreachable_from_ferros()
}

#[cfg(not(target_os = "ferros"))]
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
        .creation_time_oscillations(crate::network::time_base::now_osc())
        .signed_only_eggs(VsfType::ke(device_keypair.public.to_bytes().to_vec()), &crate::network::fgtw::fleet::envelope_slots(&device_keypair.public.to_bytes()))
        .add_section(
            "inbox_drain",
            vec![("hp".to_string(), VsfType::hP(handle_proof.to_vec()))],
        )
        .build()
        .map_err(|e| BlobError::Network(format!("Build VSF: {}", e)))?;
    let vsf_bytes = crate::network::fgtw::fleet::sign_device_envelope(unsigned, &device_keypair.public.to_bytes(), device_keypair.secret.as_bytes())
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
        out.push(FleetInboxEvent {
            kind,
            device,
            attempted_by,
            t_osc,
        });
    }
    out
}

/// Download a blob from FGTW storage (blocking version)
#[cfg(target_os = "ferros")]
pub fn get_blob_blocking(_storage_key: &str) -> Result<Option<Vec<u8>>, BlobError> {
    unreachable_from_ferros()
}

#[cfg(not(target_os = "ferros"))]
pub fn get_blob_blocking(storage_key: &str) -> Result<Option<Vec<u8>>, BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;

    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(crate::network::time_base::now_osc())
        .add_section(
            "blob_get",
            vec![("key".to_string(), VsfType::d(vsf::Ascii::new(storage_key).map_err(|e| BlobError::Network(format!("key: {e}")))?))],
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
/// Blocking `blob_delete` — the LastRites sweep needs this from `clean_device_for_reuse`, which runs on the UI thread with no runtime to await on (the async twin below is unreachable from there).
/// Uses the SHARED pooled client with a per-request deadline rather than building a private one: a fresh `Client` throws away the connection pool and the warm TLS session, which is most of a multi-second round trip on a slow uplink.
/// Idempotent, exactly like the async twin: an already-absent blob is the goal state, so `not_found` is success.
#[cfg(target_os = "ferros")]
pub fn delete_blob_blocking(_storage_key: &str, _device_keypair: &Keypair) -> Result<(), BlobError> {
    unreachable_from_ferros()
}

#[cfg(not(target_os = "ferros"))]
pub fn delete_blob_blocking(storage_key: &str, device_keypair: &Keypair) -> Result<(), BlobError> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let key_bytes = URL_SAFE_NO_PAD
        .decode(storage_key.as_bytes())
        .map_err(|e| BlobError::Network(format!("Invalid storage key: {}", e)))?;
    let key_signature = device_keypair.secret.sign(&key_bytes);

    let vsf_bytes = build_signed_blob_vsf(
        device_keypair,
        "blob_delete",
        vec![
            ("key".to_string(), VsfType::d(vsf::Ascii::new(storage_key).map_err(|e| BlobError::Network(format!("key: {e}")))?)),
            (
                "signature".to_string(),
                VsfType::ge(key_signature.to_bytes().to_vec()),
            ),
        ],
    )?;

    let response = crate::network::http::blocking_timeout(std::time::Duration::from_secs(30))
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| BlobError::Network(format!("DELETE request failed: {}", e)))?;

    let status = response.status();
    let body = response.bytes().unwrap_or_default();
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
    Ok(())
}

#[cfg(target_os = "ferros")]
pub async fn delete_blob(_storage_key: &str, _device_keypair: &Keypair) -> Result<(), BlobError> {
    unreachable_from_ferros()
}

#[cfg(not(target_os = "ferros"))]
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
            ("key".to_string(), VsfType::d(vsf::Ascii::new(storage_key).map_err(|e| BlobError::Network(format!("key: {e}")))?)),
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

    // Network smoke test against the LIVE fgtw.org worker: submit a sealed log, then find it by the handle proof it was filed under. Opening it needs the maintainer's log key, which no test holds. Run explicitly: `cargo test --features development -- --ignored roundtrip`; the device must be a member of the fleet it names.
    #[test]
    #[ignore]
    fn roundtrip_submit_list_get_decrypt() {
        let kp = super::super::derive_device_keypair(b"photonlog-smoke-fingerprint");
        let hp = [0x11u8; 32];
        let payload = b"SMOKE photon.log capability roundtrip payload".to_vec();

        put_log_blocking(&payload, "smoke note", &kp, &hp).expect("submit");

        let keys = log_list_by_proof_blocking(&hp).expect("list");
        assert!(!keys.is_empty(), "no keys listed under the proof after submit");
        let filed = parse_filed_log(&log_get_filed_blocking(&keys[0]).expect("get")).expect("parse");
        assert_eq!(filed.device, kp.public.to_bytes(), "filed under the submitting device");
    }
}

// ============================================================================ Gripes — anonymous ideas and fixes (ui/photon_app/ideas.rs, docs/ideas.md) ==========

#[cfg(target_os = "ferros")]
fn gripe_post(_section: vsf::file_format::VsfSection, _what: &str, _signer: Option<&[u8; 32]>) -> Result<Vec<u8>, BlobError> {
    unreachable_from_ferros()
}

#[cfg(not(target_os = "ferros"))]
fn gripe_post(section: vsf::file_format::VsfSection, what: &str, signer: Option<&[u8; 32]>) -> Result<Vec<u8>, BlobError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| BlobError::Network(format!("Failed to create HTTP client: {}", e)))?;
    let mut builder = vsf::VsfBuilder::new().creation_time_oscillations(crate::network::time_base::now_osc());
    if let Some(key) = signer {
        builder = builder.signed_only_eggs(VsfType::ke(ed25519_dalek::SigningKey::from_bytes(key).verifying_key().to_bytes().to_vec()), &fgtw::release::release_slots());
    }
    let unsigned = builder.add_section_direct(section).build().map_err(|e| BlobError::Network(format!("Build VSF: {}", e)))?;
    let vsf_bytes = match signer {
        Some(key) => fgtw::release::sign_release(unsigned, &fgtw::pq::SigningBundle::release(key)).map_err(|e| BlobError::Network(format!("Sign {what}: {e}")))?,
        None => unsigned,
    };
    let response = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| BlobError::Network(format!("{what} request failed: {}", e)))?;
    let bytes = response.bytes().unwrap_or_default().to_vec();
    if let Some((reason, detail)) = fgtw::client::error_frame(&bytes) {
        return Err(BlobError::ServerError(format!("{reason}: {detail}")));
    }
    Ok(bytes)
}

/// Schema-validated read of a gripe ack (vsf trust gate: the worker's bytes never meet a hand-rolled parse; `parse_document` runs the whole-document verification first).
fn gripe_ack(bytes: &[u8], schema: vsf::schema::SectionSchema) -> Result<vsf::schema::SectionBuilder, BlobError> {
    vsf::schema::SectionBuilder::parse_document(schema, bytes, None).map_err(|e| BlobError::Network(format!("Parse gripe ack: {e}")))
}

fn ack_text(sec: &vsf::schema::SectionBuilder, name: &str) -> String {
    if let Ok(s) = sec.get_value::<String>(name) {
        return s;
    }
    sec.get_value::<Vec<u8>>(name).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

/// Send a gripe: its id (blake3(text ‖ nonce), minted by the caller), the nonce, its kind ("idea" | "fix") and its text. Nothing identifying rides along.
pub fn gripe_put_blocking(id: &[u8; 32], nonce: &[u8; 32], kind: &str, text: &str) -> Result<(), BlobError> {
    let mut sec = vsf::file_format::VsfSection::new("gripe_put");
    sec.add_field_multi("id", vec![VsfType::v(b'g', id.to_vec())]);
    sec.add_field_multi("nonce", vec![VsfType::v(b'n', nonce.to_vec())]);
    sec.add_field_multi("kind", vec![VsfType::a(vsf::Ascii::new(kind).map_err(|e| BlobError::Network(format!("gripe kind: {e}")))?)]);
    sec.add_field_multi("text", vec![VsfType::v(b't', text.as_bytes().to_vec())]);
    let bytes = gripe_post(sec, "gripe_put", None)?;
    let schema = vsf::schema::SectionSchema::new("gripe_put_ack").field("id", vsf::schema::TypeConstraint::Wrapped(b'g'));
    let ack = gripe_ack(&bytes, schema)?;
    match ack.get_value::<Vec<u8>>("id") {
        Ok(b) if b.as_slice() == id => Ok(()),
        _ => Err(BlobError::ServerError("gripe_put: ack names a different id".into())),
    }
}

/// A gripe's status by id: (state, note, version). "received" until the developer sets one, "unknown" for an id the worker never saw.
pub fn gripe_get_blocking(id: &[u8; 32]) -> Result<(String, String, String), BlobError> {
    let mut sec = vsf::file_format::VsfSection::new("gripe_get");
    sec.add_field_multi("id", vec![VsfType::v(b'g', id.to_vec())]);
    let bytes = gripe_post(sec, "gripe_get", None)?;
    let schema = vsf::schema::SectionSchema::new("gripe_get_ack")
        .field("state", vsf::schema::TypeConstraint::AsciiText)
        .field("note", vsf::schema::TypeConstraint::Wrapped(b't'))
        .field("version", vsf::schema::TypeConstraint::AsciiText);
    let ack = gripe_ack(&bytes, schema)?;
    Ok((ack_text(&ack, "state"), ack_text(&ack, "note"), ack_text(&ack, "version")))
}

/// One gripe as everyone sees it (the list is public): its id, kind, the developer's state, when it arrived, its text, and the developer's note and version.
#[derive(Clone, Debug)]
pub struct PublicGripe {
    pub id: [u8; 32],
    pub kind: String,
    pub state: String,
    pub received_osc: i64,
    pub text: String,
    pub note: String,
    pub version: String,
}

/// Every gripe, public (docs/ideas.md): one typed `idea` section per gripe behind a `gripe_list_ack` head, read thru the verified-document gate.
pub fn gripe_list_blocking() -> Result<Vec<PublicGripe>, BlobError> {
    use crate::storage::record::Rec;
    let bytes = gripe_post(vsf::file_format::VsfSection::new("gripe_list"), "gripe_list", None)?;
    let sections = crate::storage::record::verified_sections(&bytes, None).map_err(|e| BlobError::Network(format!("Parse gripe list: {e}")))?;
    let Some(count) = sections.iter().find(|s| s.name == "gripe_list_ack").and_then(|s| Rec(s).uint("count")) else {
        return Err(BlobError::ServerError("gripe_list: no gripe_list_ack count".into()));
    };
    let ascii = |r: &Rec, name: &str| match r.all(name).first() {
        Some(VsfType::a(s)) | Some(VsfType::d(s)) => s.to_string(),
        _ => String::new(),
    };
    let words = |r: &Rec, name: &str| r.wrapped(name, b't').map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let ideas: Vec<PublicGripe> = sections
        .iter()
        .filter(|s| s.name == "idea")
        .map(Rec)
        .filter_map(|r| {
            Some(PublicGripe {
                id: r.h32("id")?,
                kind: ascii(&r, "kind"),
                state: ascii(&r, "state"),
                received_osc: r.osc("received").unwrap_or(0),
                text: words(&r, "text"),
                note: words(&r, "note"),
                version: ascii(&r, "version"),
            })
        })
        .collect();
    // The head says how many it sent: an idea that did not read (no 32-byte id) is a fault to see, never a row quietly missing.
    if ideas.len() as u64 != count {
        return Err(BlobError::ServerError(format!("gripe_list: the head counts {count} but {} read", ideas.len())));
    }
    Ok(ideas)
}

/// The developer sets a gripe's status: the request is signed by the release key (`signing_key`, crypto::self_verify::release_signing_key), which fgtw.org checks against the release key's public half; no token (2026-11-10).
pub fn gripe_status_blocking(signing_key: &[u8; 32], id: &[u8; 32], state: &str, note: &str, version: &str) -> Result<(), BlobError> {
    let mut sec = vsf::file_format::VsfSection::new("gripe_status");
    sec.add_field_multi("id", vec![VsfType::v(b'g', id.to_vec())]);
    sec.add_field_multi("state", vec![VsfType::a(vsf::Ascii::new(state).map_err(|e| BlobError::Network(format!("gripe state: {e}")))?)]);
    sec.add_field_multi("note", vec![VsfType::v(b't', note.as_bytes().to_vec())]);
    sec.add_field_multi("version", vec![VsfType::a(vsf::Ascii::new(version).map_err(|e| BlobError::Network(format!("gripe version: {e}")))?)]);
    let bytes = gripe_post(sec, "gripe_status", Some(signing_key))?;
    let schema = vsf::schema::SectionSchema::new("gripe_status_ack").field("state", vsf::schema::TypeConstraint::AsciiText);
    // Only the parse matters here: a gripe_status_ack that does not validate is the error, its fields are not needed.
    gripe_ack(&bytes, schema)?;
    Ok(())
}
