//! FGTW Conduit - Unified relay endpoint
//!
//! All FGTW communication goes thru POST /conduit with VSF payloads. Section name in VSF determines operation.

use vsf::VsfType;

use super::Keypair;

use crate::network::http::SEED_HTTPS as FGTW_URL;

/// The most a relayed frame may be, in bytes (security audit NET-09): the ~548 KB CLUTCH offer is the largest thing the relay carries whole, so 1 MiB holds it with room; the pipe socket and the envelope peel both refuse past it.
pub const RELAY_FRAME_LIMIT: usize = 1 << 20;

/// Peel a relay envelope received over the pipe: the whole signed `relay` VSF the SENDER built (`build_signed_vsf("relay", {recipient, payload})`, signed with their device key), which the worker now forwards intact instead of the unwrapped inner. Verifies the sender's whole-file signature, then returns `(sender_device_key, inner_payload)`. `None` on any structural/parse/verify failure — a malformed or unsigned frame off the pipe is dropped, never injected. This is the DOMAIN SEPARATOR for the pipe: a message is known-relayed because it arrived wrapped in this authenticated envelope, not because of a sentinel address. The inner payload is byte-identical to a direct message, so no inner parser changes.
pub fn peel_relay_envelope(bytes: &[u8]) -> Option<([u8; 32], Vec<u8>)> {
    use vsf::file_format::VsfHeader;

    // WHY/PROOF (security audit NET-09): any registered device may post an envelope for any pipe, and the pipe used to take a message of tungstenite's 64 MiB default — a frame past the largest thing the relay carries is refused before its signature is hashed.
    if bytes.len() > RELAY_FRAME_LIMIT {
        return None;
    }

    // Verify the sender's whole-file signature with `verify_file_signature`, NOT `read_verified`.
    // `read_verified` additionally enforces `is_original` (the header `hp` must equal the content hash), but build_signed_vsf uses `signed_only(ke)` + `sign_file`, and the same waiver every CLUTCH/chat parser takes applies here: the signature covers the ENTIRE file (authorship + integrity are proven), only the content-hp self-attestation is not asserted. Using read_verified rejected every real envelope — the "not a valid signed relay envelope" drop that black-holed the whole pipe data plane.
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

    // Resolve the section via `primary_section`, NOT a bare `VsfSection::parse`: the section NAME lives in the header TOC (near-form), so a body parse returns `section.name == ""` and a hand-rolled `== "relay"` check silently fails — the trap that black-holed the pipe (and, historically, the hub push accelerator). Pull the inner payload (v'r') off the resolved primary section.
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
        .signed_only_eggs(VsfType::ke(keypair.public.to_bytes().to_vec()), &crate::network::fgtw::fleet::envelope_slots(&keypair.public.to_bytes()))
        .add_section(section_name, fields)
        .build()
        .map_err(|e| format!("Build VSF: {}", e))?;

    // Canonical vsf signing (fills hp, then ge over BLAKE3(file, ge zeroed)) — matches the scheme the worker verifies. The old code signed the bare hp value, which the worker's file-hash verification REJECTED, so every relay send died with bad_signature (masked because relay is a non-fatal last-resort fallback).
    crate::network::fgtw::fleet::sign_device_envelope(unsigned_bytes, &keypair.public.to_bytes(), keypair.secret.as_bytes())
}


/// Answer the seed's pipe challenge, the first frame on a fresh pipe (fgtw::pipe's open handshake): the seed binds our pipe only once this answer, signed with our device key, comes back up the socket, and until then no frame for us goes down it.
/// `None` when `frame` is not a challenge the seed signed (a relay envelope, or a worker from before the handshake), so the caller reads it as an ordinary frame.
/// `Some(Err)` is a seed challenge we will not answer: one issued for another device or another service's hub.
pub fn pipe_answer(frame: &[u8], keypair: &Keypair) -> Option<Result<Vec<u8>, String>> {
    pipe_answer_under(frame, &crate::network::fgtw::bootstrap::FGTW_ED25519_PUBLIC_KEY, keypair)
}

/// [`pipe_answer`] under a given seed key, so a test can stand in for the seed.
fn pipe_answer_under(frame: &[u8], seed_public: &[u8; 32], keypair: &Keypair) -> Option<Result<Vec<u8>, String>> {
    let challenge = fgtw::pipe::read_challenge(frame, seed_public)?;
    if challenge.dev != keypair.public.to_bytes() {
        return Some(Err("pipe challenge names another device".to_string()));
    }
    if challenge.svc.is_some() {
        return Some(Err("pipe challenge is for another service's hub".to_string()));
    }
    // The installed device signer when this is our device key, the same one every device envelope is signed with.
    let signer = crate::network::fgtw::fleet::signer_for(keypair);
    Some(fgtw::pipe::open_answer(frame, &signer, vsf::eagle_time_oscillations()))
}

/// Send a message via FGTW conduit relay
///
/// # Arguments
/// * `keypair` - Our device keypair for signing * `recipient_pubkey` - Recipient's device public key (32 bytes) * `message_bytes` - Already-encrypted message (VSF format)
///
/// # Returns
/// Ok(()) on success, Err with message on failure
/// Process-wide relay refusal list (treat-as-stolen lockout): every relay send — chat, ping, ceremony, history, ack, present and future — passes thru the two senders below, so this ONE gate guarantees a locked device never receives another relay frame no matter which path built the target list. Updated by the app on lock edges and on every locked-set adopt; read-only here.
static LOCKED_RELAY_TARGETS: std::sync::Mutex<Vec<[u8; 32]>> = std::sync::Mutex::new(Vec::new());

pub fn set_locked_relay_targets(locked: Vec<[u8; 32]>) {
    *LOCKED_RELAY_TARGETS.lock().unwrap() = locked;
}

/// Devices the relay last answered "recipient's pipe is closed" for: it has no mailbox, so every later frame to them is discarded the same way.
/// A send to one is answered here without the round trip, until the device is heard from again ([`note_alive`]: a frame off its pipe, a pong, its address push) — an edge, never a timer (Nick 2026-11-07: 571 of 1,638 relay posts in six hours went to closed pipes).
static PIPE_CLOSED: std::sync::Mutex<Vec<[u8; 32]>> = std::sync::Mutex::new(Vec::new());

/// The device was just heard from: its pipe may be open again, so the next relay send to it is tried for real.
pub fn note_alive(device: &[u8; 32]) {
    PIPE_CLOSED.lock().unwrap().retain(|d| d != device);
}

fn pipe_known_closed(recipient: &[u8; 32]) -> bool {
    PIPE_CLOSED.lock().unwrap().iter().any(|d| d == recipient)
}

fn note_closed(recipient: &[u8; 32]) {
    let mut closed = PIPE_CLOSED.lock().unwrap();
    if !closed.contains(recipient) {
        closed.push(*recipient);
    }
}

fn relay_target_locked(recipient: &[u8; 32]) -> bool {
    LOCKED_RELAY_TARGETS
        .lock()
        .unwrap()
        .iter()
        .any(|d| d == recipient)
}

/// The relay is fgtw.org; ferros does not speak to it — a peer with no direct path is out of reach from the phone for now.
#[cfg(target_os = "ferros")]
pub async fn send_via_relay(_keypair: &Keypair, _recipient_pubkey: &[u8; 32], _message_bytes: &[u8]) -> Result<(), String> {
    Err("no relay from ferros".to_string())
}

#[cfg(target_os = "ferros")]
pub async fn send_via_relay_always(_keypair: &Keypair, _recipient_pubkey: &[u8; 32], _message_bytes: &[u8]) -> Result<(), String> {
    Err("no relay from ferros".to_string())
}

/// A frame someone is waiting on (a chat, its ACK, a ceremony step, a wave signal, the transport's own fallback): tried even when the relay last said the device had no open pipe.
/// The device may have come back without sending us anything (a phone woken in a pocket), and only a send learns it; background traffic waits for the device to be heard from instead.
#[cfg(not(target_os = "ferros"))]
pub async fn send_via_relay_always(keypair: &Keypair, recipient_pubkey: &[u8; 32], message_bytes: &[u8]) -> Result<(), String> {
    relay_send(keypair, recipient_pubkey, message_bytes, false).await
}

/// Background traffic (a ping, a history page, a phonebook push): skipped while the relay's last word for the device was "no open pipe".
#[cfg(not(target_os = "ferros"))]
pub async fn send_via_relay(keypair: &Keypair, recipient_pubkey: &[u8; 32], message_bytes: &[u8]) -> Result<(), String> {
    relay_send(keypair, recipient_pubkey, message_bytes, true).await
}

#[cfg(not(target_os = "ferros"))]
async fn relay_send(
    keypair: &Keypair,
    recipient_pubkey: &[u8; 32],
    message_bytes: &[u8],
    honour_closed: bool,
) -> Result<(), String> {
    // A fan-out list can carry our own device after an echo-poisoned era (an own frame looping back elected us a contact's endpoint, field 2026-08-12) — relaying to ourselves just manufactures the next echo, so refuse it at the one seam every relay send passes thru.
    if recipient_pubkey == keypair.public.as_bytes() {
        return Err("relay: recipient is OUR OWN device — echo refused".to_string());
    }
    if relay_target_locked(recipient_pubkey) {
        return Err("relay: recipient is locked out".to_string());
    }
    // The same answer the relay would give, without asking it again: the verdict stands until the device is heard from.
    if honour_closed && pipe_known_closed(recipient_pubkey) {
        return Err("relay: recipient offline (frame discarded)".to_string());
    }
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
    let reached = relay_reached(&body);
    crate::logf!(
        "RELAY: {} for {}... ({} bytes)",
        if reached {
            "delivered"
        } else {
            "DROPPED — recipient's pipe is closed, no mailbox to hold it"
        },
        hex::encode(&recipient_pubkey[..4]),
        message_bytes.len()
    );
    if !reached {
        note_closed(recipient_pubkey);
        return Err("relay: recipient offline (frame discarded)".to_string());
    }
    Ok(())
}

/// The worker's per-frame delivery verdict, parsed from `relay_ack`. `true` = handed to a live socket, `false` = the recipient had no pipe open and the frame was DISCARDED (there is no mailbox). Absent field = an older worker; treat as delivered so a mixed fleet never silently stops relaying.
///
/// Use this ONLY to decide whether to retry or defer THIS send. Presence stays a receive-side fact — a pong or ACK arriving over our pipe — never a send-time inference, or two authorities start disagreeing about who is online.
fn relay_reached(body: &[u8]) -> bool {
    let schema = vsf::schema::SectionSchema::new("relay_ack")
        .field("reach", vsf::schema::TypeConstraint::Any);
    match vsf::schema::SectionBuilder::parse_document(schema, body, None) {
        Ok(sec) => sec
            .get_fields("reach")
            .first()
            .and_then(|f| f.values.first())
            .and_then(|v| v.as_u64())
            .map_or(true, |n| n != 0),
        Err(_) => true,
    }
}

/// Synchronous version of send_via_relay for non-async contexts
#[cfg(target_os = "ferros")]
pub fn send_via_relay_sync(_keypair: &Keypair, _recipient_pubkey: &[u8; 32], _message_bytes: &[u8]) -> Result<(), String> {
    Err("no relay from ferros".to_string())
}

#[cfg(not(target_os = "ferros"))]
pub fn send_via_relay_sync(
    keypair: &Keypair,
    recipient_pubkey: &[u8; 32],
    message_bytes: &[u8],
) -> Result<(), String> {
    // Same self-recipient refusal as the async path — see send_via_relay.
    if recipient_pubkey == keypair.public.as_bytes() {
        return Err("relay: recipient is OUR OWN device — echo refused".to_string());
    }
    if relay_target_locked(recipient_pubkey) {
        return Err("relay: recipient is locked out".to_string());
    }
    if pipe_known_closed(recipient_pubkey) {
        return Err("relay: recipient offline (frame discarded)".to_string());
    }
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
    let reached = relay_reached(&body);
    crate::logf!(
        "RELAY: {} for {}... ({} bytes)",
        if reached {
            "delivered"
        } else {
            "DROPPED — recipient's pipe is closed, no mailbox to hold it"
        },
        hex::encode(&recipient_pubkey[..4]),
        message_bytes.len()
    );
    if !reached {
        note_closed(recipient_pubkey);
        return Err("relay: recipient offline (frame discarded)".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod peel_tests {
    use super::*;

    /// A `send_via_relay` envelope must round-trip thru `peel_relay_envelope`. This guards the TOC-name trap specifically: the section name lives in the header near-form, so a bare body parse sees `name == ""` and any `== "relay"` check fails — which silently black-holed the whole pipe data plane until caught in a log.
    /// NET-09: an envelope past the relay frame limit is refused before its signature is hashed, however well it is signed.
    #[test]
    fn peel_refuses_a_frame_past_the_limit() {
        let kp = crate::network::fgtw::Keypair::from_seed(&[4u8; 32]);
        let inner = vec![1u8; RELAY_FRAME_LIMIT];
        let envelope = build_signed_vsf(
            &kp,
            "relay",
            vec![
                ("recipient".to_string(), VsfType::kx([7u8; 32].to_vec())),
                ("payload".to_string(), VsfType::v(b'r', inner)),
            ],
        )
        .expect("build envelope");
        assert!(envelope.len() > RELAY_FRAME_LIMIT);
        assert!(peel_relay_envelope(&envelope).is_none());
    }

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
        assert_eq!(
            sender,
            kp.public.to_bytes(),
            "sender key must be the signer"
        );
        assert_eq!(
            payload, inner,
            "inner payload must round-trip byte-identical"
        );
    }
}

#[cfg(test)]
mod pipe_answer_tests {
    use super::*;

    const SEED_SECRET: [u8; 32] = [0x6a; 32];

    fn seed_public() -> [u8; 32] {
        ed25519_dalek::SigningKey::from_bytes(&SEED_SECRET).verifying_key().to_bytes()
    }

    /// The answer is a `pipe_open` frame signed by our device key, committing to the challenge's exact bytes under the "photon pipe open" domain, which is what the seed checks before it binds the pipe.
    #[test]
    fn the_answer_is_a_device_signed_pipe_open_bound_to_the_challenge() {
        let kp = crate::network::fgtw::Keypair::from_seed(&[5u8; 32]);
        let challenge = fgtw::pipe::challenge(&SEED_SECRET, &kp.public.to_bytes(), None, &[1u8; 32], vsf::eagle_time_oscillations()).unwrap();
        let answer = pipe_answer_under(&challenge, &seed_public(), &kp).expect("a seed challenge").expect("answered");
        assert_eq!(fgtw::pipe::check_answer(&answer, &fgtw::pipe::open_binding(&challenge)), Some(kp.public.to_bytes()));
        // The encoding, field by field: header signed by our device key, primary section `pipe_open`, one `ch` hash equal to the domain-separated commitment.
        let (header, end) = vsf::verification::read_verified(&answer, Some(kp.public.to_bytes())).expect("verifies under our key");
        let section = header.primary_section(&answer, end).unwrap();
        assert_eq!(section.name, fgtw::pipe::OPEN_SECTION);
        let ch = section.get_field("ch").and_then(|f| f.values.first()).cloned();
        let expected = *blake3::Hasher::new_derive_key("photon pipe open").update(&challenge).finalize().as_bytes();
        assert_eq!(ch, Some(VsfType::hb(expected.to_vec())));
        // It is never mistaken for a relay frame on either end.
        assert!(peel_relay_envelope(&answer).is_none());
        assert!(peel_relay_envelope(&challenge).is_none());
    }

    /// A challenge for another device or another service's hub is not answered, and a frame no seed signed is not a challenge at all.
    #[test]
    fn only_our_own_seed_signed_challenge_is_answered() {
        let kp = crate::network::fgtw::Keypair::from_seed(&[6u8; 32]);
        let other = crate::network::fgtw::Keypair::from_seed(&[7u8; 32]);
        let now = vsf::eagle_time_oscillations();
        let for_other = fgtw::pipe::challenge(&SEED_SECRET, &other.public.to_bytes(), None, &[2u8; 32], now).unwrap();
        assert!(matches!(pipe_answer_under(&for_other, &seed_public(), &kp), Some(Err(_))));
        let for_rd = fgtw::pipe::challenge(&SEED_SECRET, &kp.public.to_bytes(), Some(fgtw::pipe::SVC_RUSTDESK), &[3u8; 32], now).unwrap();
        assert!(matches!(pipe_answer_under(&for_rd, &seed_public(), &kp), Some(Err(_))));
        let forged = fgtw::pipe::challenge(&[0x11; 32], &kp.public.to_bytes(), None, &[4u8; 32], now).unwrap();
        assert!(pipe_answer_under(&forged, &seed_public(), &kp).is_none(), "a challenge under any other key is not the seed's");
        assert!(pipe_answer(b"", &kp).is_none());
    }
}
