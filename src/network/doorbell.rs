//! Doorbell client — ring a dozed peer + publish this device's wake bells (docs/reachability-doorbell.md).
//!
//! The ring is a remote `wake()` for a peer whose process the OS has stopped scheduling: the worker relays an EMPTY high-priority push to the peer's published bell (FCM / UnifiedPush), the woken phone re-punches its NAT hole, and the sender's normal retransmit delivers DIRECTLY — content never rides the bell, so Google learns only that a wake happened and when.
//! Everything here is fire-and-forget off-thread (blocking reqwest on a spawned thread, mirroring the blob/log submit paths): a ring is a rare escalation and must never stall the UI tick that decided to send it. The worker debounces per-target, so an over-eager caller costs an HTTP round trip, not a wake.

use vsf::VsfType;

const FGTW_URL: &str = "https://fgtw.org";

/// Provenance-only signed frame (the log_put/blob_put shape): ke in the header names the signer, the canonical hp+ge are filled for wire hygiene, and the op-specific authorization is the DETACHED `signature` field each op defines — that's what the worker verifies.
fn signed_frame(
    keypair: &crate::network::fgtw::Keypair,
    section_name: &str,
    fields: Vec<(String, VsfType)>,
) -> Result<Vec<u8>, String> {
    let unsigned = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only(VsfType::ke(keypair.public.as_bytes().to_vec()))
        .add_section(section_name, fields)
        .build()
        .map_err(|e| format!("build VSF: {}", e))?;
    let hash = vsf::verification::compute_provenance_hash(&unsigned).map_err(|e| format!("hash: {}", e))?;
    let signature = keypair.sign(&hash);
    let mut signed = unsigned;
    vsf::verification::fill_provenance_hash(&mut signed, &hash).map_err(|e| format!("fill hash: {}", e))?;
    vsf::verification::fill_signature(&mut signed, &signature.to_bytes()).map_err(|e| format!("fill sig: {}", e))?;
    Ok(signed)
}

fn post(vsf_bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("client: {}", e))?;
    let resp = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(vsf_bytes)
        .send()
        .map_err(|e| format!("send: {}", e))?;
    let status = resp.status();
    let body = resp.bytes().map_err(|e| format!("body: {}", e))?.to_vec();
    if let Some((reason, detail)) = fgtw::client::error_frame(&body) {
        return Err(format!("{}: {}", reason, detail));
    }
    if !status.is_success() {
        return Err(format!("transport {}", status));
    }
    Ok(body)
}

/// Schema-validated read of `ring_ack.rung` (vsf trust gate: network bytes never meet a hand-rolled parse).
fn ring_ack_rung(body: &[u8]) -> Option<String> {
    let schema = vsf::schema::SectionSchema::new("ring_ack")
        .field("rung", vsf::schema::TypeConstraint::Wrapped(b'r'));
    let section = vsf::schema::SectionBuilder::parse_document(schema, body, None).ok()?;
    let bytes = section.get_value::<Vec<u8>>("rung").ok()?;
    String::from_utf8(bytes).ok()
}

/// Ring `target` (a friend's or sibling's handle_proof) — one wake, debounced worker-side. Detached signature over `b"ring" ‖ target ‖ ts_le8`.
pub fn spawn_ring(device_secret: [u8; 32], target: [u8; 32]) {
    std::thread::spawn(move || {
        // The stored device secret IS the ed25519 seed, so this reproduces the exact device keypair.
        let keypair = crate::network::fgtw::Keypair::from_seed(&device_secret);
        let ts = vsf::eagle_time_oscillations();
        let mut msg = b"ring".to_vec();
        msg.extend_from_slice(&target);
        msg.extend_from_slice(&ts.to_le_bytes());
        let signature = keypair.sign(&msg);
        let fields = vec![
            ("target".to_string(), VsfType::v(b'r', target.to_vec())),
            ("timestamp".to_string(), VsfType::e(vsf::types::EtType::e6(ts))),
            ("signature".to_string(), VsfType::ge(signature.to_bytes().to_vec())),
        ];
        match signed_frame(&keypair, "ring", fields).and_then(post) {
            Ok(body) => {
                let rung = ring_ack_rung(&body).unwrap_or_else(|| "?".to_string());
                crate::logf!("DOORBELL: rang {} — {}", crate::fp(&target).as_str(), rung);
            }
            Err(e) => crate::logf!("DOORBELL: ring {} failed: {}", crate::fp(&target).as_str(), e),
        }
    });
}

/// Publish this device's preference-ordered bell list under OUR handle_proof. Detached signature over `hp ‖ ts_le8 ‖ bell0 ‖ \n ‖ bell1 …`.
pub fn spawn_publish_bells(device_secret: [u8; 32], hp: [u8; 32], bells: Vec<String>) {
    std::thread::spawn(move || {
        let keypair = crate::network::fgtw::Keypair::from_seed(&device_secret);
        let ts = vsf::eagle_time_oscillations();
        let mut msg = Vec::new();
        msg.extend_from_slice(&hp);
        msg.extend_from_slice(&ts.to_le_bytes());
        for (i, b) in bells.iter().enumerate() {
            if i > 0 {
                msg.push(b'\n');
            }
            msg.extend_from_slice(b.as_bytes());
        }
        let signature = keypair.sign(&msg);
        let mut fields = vec![
            ("hp".to_string(), VsfType::v(b'r', hp.to_vec())),
            ("timestamp".to_string(), VsfType::e(vsf::types::EtType::e6(ts))),
            ("signature".to_string(), VsfType::ge(signature.to_bytes().to_vec())),
        ];
        // One `bell` per repeated field, preference order — the instance is identified by its POSITION, never a decimal-suffixed name; the worker reads all of them via get_fields.
        for b in &bells {
            fields.push(("bell".to_string(), VsfType::v(b'r', b.clone().into_bytes())));
        }
        match signed_frame(&keypair, "bell_put", fields).and_then(post) {
            Ok(_) => crate::logf!("DOORBELL: published {} bell(s)", bells.len()),
            Err(e) => crate::logf!("DOORBELL: bell publish failed: {}", e),
        }
    });
}
