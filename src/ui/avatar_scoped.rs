//! Avatar publishing and fetching over SCOPED BLOBS (docs/scoped-blobs.md) — the replacement for the bearer pin.
//!
//! One ciphertext, encrypted under a random data key. That key is wrapped separately for each reader into a private slot at an address only the two parties can derive from a secret they already share: the CLUTCH pair secret for a friend, the fleet key for our own devices, and a device-derived self key so a lone device works before any ceremony.
//!
//! What this fixes, concretely, from one day of live failures (2026-08-01): the pin was simultaneously the address and the key, so losing it orphaned the avatar (twice), leaking it granted everything, and there were TWO ciphertexts of one image keyed differently — which is what made a direct-served avatar fail to decrypt and made own-avatar recovery search an address nothing had written since the pin went random.
//!
//! Here the blob id is not a secret and nobody's slot reveals anyone else's, so the object leaks a reader COUNT at publish time and nothing more.

use fgtw::scoped_blob::{
    is_vsf, new_blob_id, new_dek, open_content, open_slot, seal_content_doc, slot_writes, SlotContents,
};

use crate::network::fgtw::blob;
use crate::network::fgtw::Keypair;

/// Blobs are addressed by base64url of a 32-byte id — both the content and each slot.
fn addr(id: &[u8; 32]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(id)
}

/// The base purpose tag; the wire purpose is [`avatar_purpose`] — base ‖ publisher pid, never this alone.
const AVATAR_PURPOSE_BASE: &[u8] = b"avatar";

/// The purpose naming ONE identity's avatar: the base tag with the PUBLISHER's party id appended (binary).
///
/// The publisher must be in the address because a CLUTCH pair secret is ONE secret shared by both directions: with a bare "avatar" purpose, both sides' publishes derived the SAME slot address from their shared pair, the last writer won, and a device opened its friend's slot to find its OWN face (2026-08-02). Every purpose under a symmetric reader secret must carry its publisher — attachments included when they arrive.
pub fn avatar_purpose(publisher: &[u8; 32]) -> Vec<u8> {
    let mut p = Vec::with_capacity(AVATAR_PURPOSE_BASE.len() + publisher.len());
    p.extend_from_slice(AVATAR_PURPOSE_BASE);
    p.extend_from_slice(publisher);
    p
}

/// Publish `plaintext` so exactly `readers` can fetch it — one content upload plus one tiny slot write each.
///
/// `readers` is the shared secret per reader, NOT their identity: the CLUTCH pair secret for each friend, the fleet key for our own devices, our self key for this device. Removing a reader means calling this again without them; the fresh data key is what puts the new version beyond their reach.
pub fn publish_blocking(
    plaintext: &[u8],
    readers: &[[u8; 32]],
    purpose: &[u8],
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
) -> Result<([u8; 32], SlotContents), String> {
    let dek = new_dek();
    let blob_id = new_blob_id();
    // Stored as a signed VSF document (docs/scoped-blobs.md), never bare bytes.
    let sealed = seal_content_doc(&dek, &blob_id, plaintext, device_keypair)?;

    // Content first: a slot pointing at an object that does not exist yet would strand any reader who acts on it immediately.
    blob::put_blob_blocking(&addr(&blob_id), &sealed, device_keypair, handle_proof)
        .map_err(|e| format!("scoped: content upload failed: {}", e))?;

    let contents = SlotContents { blob_id, dek };
    let writes = slot_writes(readers, purpose, &contents, device_keypair)?;
    let total = writes.len();
    let mut written = 0usize;
    for w in &writes {
        match blob::put_blob_blocking(&addr(&w.address), &w.sealed, device_keypair, handle_proof) {
            Ok(()) => written += 1,
            // One unreachable slot must not abort the rest: every OTHER reader's access is independent, and this one is repaired by the next publish rather than by unwinding a good upload.
            Err(e) => crate::logf!("SCOPED: slot write failed (one reader): {}", e),
        }
    }
    crate::logf!(
        "SCOPED: published {} bytes, {}/{} reader slot(s) written",
        sealed.len(),
        written,
        total
    );
    Ok((blob_id, contents))
}

/// Where our own published blob's identity lives, so readers can be ADDED later without re-uploading the content. Without this a new reader can only be served by a full republish, which is both wasteful and — worse — impossible to do correctly, since the DEK is gone the moment `publish_blocking` returns.
fn published_key(purpose: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"PHOTON_SCOPED_PUBLISHED_v");
    h.update(&[fgtw::scoped_blob::SCOPED_VERSION]);
    h.update(purpose);
    crate::storage::vault_key("scoped_published", h.finalize().as_bytes())
}

/// Remember what we published, so a reader who arrives later gets a slot rather than a republish.
pub fn remember_published(
    purpose: &[u8],
    contents: &SlotContents,
    storage: &crate::storage::FlatStorage,
) {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(&contents.blob_id);
    buf[32..].copy_from_slice(&contents.dek);
    if let Err(e) = storage.write_addr(&published_key(purpose), &buf) {
        crate::logf!("SCOPED: could not record what we published: {}", e);
    }
}

/// Grant ONE new reader access to what we already published — a single ~80 byte slot write against the existing ciphertext.
///
/// This is what makes a rotating reader secret survivable. A slot address is derived from the CLUTCH pair secret, and that secret is re-minted by every re-clutch — so a friend who re-clutches after we published starts looking at a new address where nothing was ever written, and their avatar silently stops resolving. Re-granting on the mint edge puts a slot at the new address without touching the content.
pub fn grant_reader(
    kek_secret: &[u8; 32],
    purpose: &[u8],
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
    storage: &crate::storage::FlatStorage,
) {
    let Ok(Some(buf)) = storage.read_addr(&published_key(purpose)) else {
        return; // nothing published yet — the next publish will include them
    };
    if buf.len() != 64 {
        return;
    }
    let mut contents = SlotContents {
        blob_id: [0u8; 32],
        dek: [0u8; 32],
    };
    contents.blob_id.copy_from_slice(&buf[..32]);
    contents.dek.copy_from_slice(&buf[32..]);
    let Ok(writes) = slot_writes(std::slice::from_ref(kek_secret), purpose, &contents, device_keypair) else {
        return;
    };
    for w in &writes {
        match blob::put_blob_blocking(&addr(&w.address), &w.sealed, device_keypair, handle_proof) {
            Ok(()) => crate::log("SCOPED: granted a reader access to the existing blob"),
            Err(e) => crate::logf!("SCOPED: grant failed: {}", e),
        }
    }
}

/// Fetch content shared with us: find our slot from the secret we share with the publisher, unwrap the key, pull the one ciphertext.
///
/// `None` means "nothing shared with us here" — an absent slot, a slot we cannot open, or content that has since been republished elsewhere. All three are the same fact to a reader, and none is an error worth surfacing.
pub fn fetch_blocking(kek_secret: &[u8; 32], purpose: &[u8]) -> Option<Vec<u8>> {
    let slot_addr = fgtw::scoped_blob::slot_address(kek_secret, purpose);
    let sealed_slot = blob::get_blob_blocking(&addr(&slot_addr)).ok().flatten()?;
    let contents = open_slot(kek_secret, purpose, &sealed_slot)?;
    let sealed_content = blob::get_blob_blocking(&addr(&contents.blob_id))
        .ok()
        .flatten()?;
    open_content(&contents.dek, &contents.blob_id, &sealed_content)
}

/// The avatar's plaintext: the house image (`vsf::builders::compressed_image_encoded`), AV1 pixels with their encoding: untagged VSF RGB for an LDR source, `g2t` for an HDR one.
pub fn avatar_plaintext(av1: Vec<u8>, encoding: vsf::visual::Encoding) -> Result<Vec<u8>, String> {
    vsf::builders::compressed_image_encoded(av1, encoding)
}

/// A scoped avatar's plaintext → (size, display pixels): the VSF image (its tag honoured for an SDR screen), or bare AV1 from a publisher that predates it. `None` for anything else, such as the pin-sealed wall form that shares the cache slot.
pub fn decode_avatar_plaintext(bytes: &[u8]) -> Option<(usize, Vec<u8>)> {
    if !is_vsf(bytes) {
        return crate::ui::avatar::decode_avatar_av1_to_display(bytes);
    }
    let parsed = vsf::builders::parse_compressed_image(bytes).ok()?;
    if parsed.encoding != vsf::builders::ENCODING_AV1 {
        return None;
    }
    let (w, h, mut rgb) = crate::ui::avatar::decode_avatar(&parsed.data).ok()?;
    crate::ui::attach_preview::display_codes(&mut rgb, parsed.colour_encoding);
    (w == h).then_some((w, rgb))
}

/// Convert in place: if what we last published under `purpose` is still the bare pre-envelope form, republish it signed and framed to the current reader set. Old readers reject the new form; new readers read both. Blocking — call off-thread. No-op when nothing was published, when it is already a VSF document, when there are no readers, or when there is no vault copy to republish.
pub fn upgrade_in_place(
    purpose: &[u8],
    readers: &[[u8; 32]],
    own_av1: Option<Vec<u8>>,
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
    storage: &crate::storage::FlatStorage,
) {
    let Ok(Some(buf)) = storage.read_addr(&published_key(purpose)) else {
        return;
    };
    if buf.len() != 64 || readers.is_empty() {
        return;
    }
    let blob_id: [u8; 32] = buf[..32].try_into().expect("64-byte record");
    let Ok(Some(stored)) = blob::get_blob_blocking(&addr(&blob_id)) else {
        return;
    };
    if is_vsf(&stored) {
        return;
    }
    let Some(av1) = own_av1 else {
        crate::log("SCOPED: bare avatar blob, but no vault copy to republish it from");
        return;
    };
    let plaintext = match avatar_plaintext(av1, vsf::visual::Encoding::Gamma2) {
        Ok(p) => p,
        Err(e) => return crate::logf!("SCOPED: upgrade skipped: {}", e),
    };
    match publish_blocking(&plaintext, readers, purpose, device_keypair, handle_proof) {
        Ok((_, contents)) => {
            remember_published(purpose, &contents, storage);
            crate::log("SCOPED: republished our avatar as a signed VSF document (convert in place)");
        }
        Err(e) => crate::logf!("SCOPED: upgrade republish failed: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both plaintext forms decode: the house VSF image, and bare AV1 from before it.
    #[test]
    fn avatar_plaintext_round_trips_and_bare_av1_still_reads() {
        let rgb: Vec<f32> = (0..64 * 64 * 3).map(|i| (i % 7) as f32 / 7.0).collect();
        let av1 = crate::ui::avatar::encode_av1_wh(&rgb, 64, 64, 40).unwrap();
        let plain = avatar_plaintext(av1.clone(), vsf::visual::Encoding::Gamma2).unwrap();
        assert!(is_vsf(&plain));
        let (size, px) = decode_avatar_plaintext(&plain).expect("VSF form");
        let (bsize, bpx) = decode_avatar_plaintext(&av1).expect("bare form");
        assert_eq!((size, px.len()), (64, 64 * 64 * 3));
        assert_eq!((size, &px), (bsize, &bpx), "an untagged image decodes exactly like bare AV1");
    }

    /// The address a reader derives must be exactly the one the publisher writes — the two sides compute it independently from the same secret, and a mismatch would silently share with nobody.
    #[test]
    fn reader_and_publisher_agree_on_the_slot_address() {
        let secret = [7u8; 32];
        let purpose = avatar_purpose(&[5u8; 32]);
        let contents = SlotContents {
            blob_id: [1u8; 32],
            dek: [2u8; 32],
        };
        let writes = slot_writes(&[secret], &purpose, &contents, &Keypair::from_seed(&[9u8; 32])).unwrap();
        assert_eq!(
            writes[0].address,
            fgtw::scoped_blob::slot_address(&secret, &purpose)
        );
        // And the address is stable across calls, so an update overwrites in place rather than stranding the reader.
        assert_eq!(
            fgtw::scoped_blob::slot_address(&secret, &purpose),
            fgtw::scoped_blob::slot_address(&secret, &purpose)
        );
    }

    /// The bug this module shipped with: two publishers sharing ONE pair secret must not share a slot. The publisher pid in the purpose is what separates them — under a bare tag both sides derived one address and the last publish overwrote the other's avatar.
    #[test]
    fn two_publishers_on_one_pair_secret_get_distinct_slots() {
        let pair_secret = [7u8; 32];
        let pub_a = avatar_purpose(&[1u8; 32]);
        let pub_b = avatar_purpose(&[2u8; 32]);
        assert_ne!(
            fgtw::scoped_blob::slot_address(&pair_secret, &pub_a),
            fgtw::scoped_blob::slot_address(&pair_secret, &pub_b)
        );
    }
}
