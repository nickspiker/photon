//! Avatar publishing and fetching over SCOPED BLOBS (docs/scoped-blobs.md) — the replacement for the bearer pin.
//!
//! One ciphertext, encrypted under a random data key. That key is wrapped separately for each reader into a private slot at an address only the two parties can derive from a secret they already share: the CLUTCH pair secret for a friend, the fleet key for our own devices, and a device-derived self key so a lone device works before any ceremony.
//!
//! What this fixes, concretely, from one day of live failures (2026-08-01): the pin was simultaneously the address and the key, so losing it orphaned the avatar (twice), leaking it granted everything, and there were TWO ciphertexts of one image keyed differently — which is what made a direct-served avatar fail to decrypt and made own-avatar recovery search an address nothing had written since the pin went random.
//!
//! Here the blob id is not a secret and nobody's slot reveals anyone else's, so the object leaks a reader COUNT at publish time and nothing more.

use fgtw::scoped_blob::{
    new_blob_id, new_dek, open_content, open_slot, seal_content, slot_writes, SlotContents,
};

use crate::network::fgtw::blob;
use crate::network::fgtw::Keypair;

/// Blobs are addressed by base64url of a 32-byte id — both the content and each slot.
fn addr(id: &[u8; 32]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(id)
}

/// The purpose tag separating this blob from any other sharing the same reader secret. Attachments will pass their own id here; the avatar is a singleton per identity.
pub const AVATAR_PURPOSE: &[u8] = b"avatar";

/// Publish `plaintext` so exactly `readers` can fetch it — one content upload plus one tiny slot write each.
///
/// `readers` is the shared secret per reader, NOT their identity: the CLUTCH pair secret for each friend, the fleet key for our own devices, our self key for this device. Removing a reader means calling this again without them; the fresh data key is what puts the new version beyond their reach.
pub fn publish_blocking(
    plaintext: &[u8],
    readers: &[[u8; 32]],
    purpose: &[u8],
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
) -> Result<[u8; 32], String> {
    let dek = new_dek();
    let blob_id = new_blob_id();
    let sealed = seal_content(&dek, &blob_id, plaintext)?;

    // Content first: a slot pointing at an object that does not exist yet would strand any reader who acts on it immediately.
    blob::put_blob_blocking(&addr(&blob_id), &sealed, device_keypair, handle_proof)
        .map_err(|e| format!("scoped: content upload failed: {}", e))?;

    let contents = SlotContents { blob_id, dek };
    let writes = slot_writes(readers, purpose, &contents)?;
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
    Ok(blob_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The address a reader derives must be exactly the one the publisher writes — the two sides compute it independently from the same secret, and a mismatch would silently share with nobody.
    #[test]
    fn reader_and_publisher_agree_on_the_slot_address() {
        let secret = [7u8; 32];
        let contents = SlotContents {
            blob_id: [1u8; 32],
            dek: [2u8; 32],
        };
        let writes = slot_writes(&[secret], AVATAR_PURPOSE, &contents).unwrap();
        assert_eq!(
            writes[0].address,
            fgtw::scoped_blob::slot_address(&secret, AVATAR_PURPOSE)
        );
        // And the address is stable across calls, so an update overwrites in place rather than stranding the reader.
        assert_eq!(
            fgtw::scoped_blob::slot_address(&secret, AVATAR_PURPOSE),
            fgtw::scoped_blob::slot_address(&secret, AVATAR_PURPOSE)
        );
    }
}
