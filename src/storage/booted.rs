//! The BOOTED LEDGER — every contact this identity has booted, kept so the removal STICKS (Nick 2026-09-27, Theresa's contact that "still persisted").
//!
//! A boot used to live only in the fleet roster slot as one tombstone: a failed push was never retried, a re-seal dropped it, and the cloud contacts backup or a sibling's fresh stamp re-added the contact fleet-wide.
//! The ledger is this device's own memory of each boot: every roster push re-emits it, and every path that can mint a contact (roster merge, cloud merge, chain adopt) consults it first.
//! A deliberate re-add stamps newer than the boot and clears the entry.
//! One vault entry at `vault_key("booted", vault_seed)`, a complete VSF document with one `booted` section per contact.

use crate::storage::record::{verified_sections, Rec};
use crate::storage::{FlatStorage, StorageError};
use vsf::VsfType;

const SECTION: &str = "booted";

/// One booted contact: who, and when the boot happened (the stamp a re-add must beat).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Booted {
    pub handle_proof: [u8; 32],
    /// The contact's party id — what chain participants and the roster key on.
    pub party_id: [u8; 32],
    /// Eagle time of the boot.
    pub at: i64,
}

fn booted_addr(storage: &FlatStorage) -> [u8; 32] {
    crate::storage::vault_key("booted", &storage.vault_seed())
}

/// Encode the ledger as one complete VSF document (header, provenance hash, one section per booted contact).
pub fn booted_to_vsf_bytes(list: &[Booted]) -> Result<Vec<u8>, StorageError> {
    let mut b = vsf::VsfBuilder::new().creation_time_oscillations(vsf::eagle_time_oscillations()).provenance_only();
    for x in list {
        b = b.add_section(
            SECTION,
            vec![
                ("hp".into(), VsfType::hb(x.handle_proof.to_vec())),
                ("pid".into(), VsfType::hb(x.party_id.to_vec())),
                ("at".into(), VsfType::e(vsf::types::EtType::e6(x.at))),
            ],
        );
    }
    b.build().map_err(StorageError::Parse)
}

/// Decode the ledger — a verified read; a record missing any field is dropped whole, never half-applied.
pub fn booted_from_vsf_bytes(bytes: &[u8]) -> Result<Vec<Booted>, StorageError> {
    Ok(verified_sections(bytes, None)?
        .iter()
        .filter(|s| s.name == SECTION)
        .filter_map(|s| {
            let r = Rec(s);
            Some(Booted { handle_proof: r.h32("hp")?, party_id: r.h32("pid")?, at: r.osc("at")? })
        })
        .collect())
}

/// The stored ledger; none stored reads as empty.
pub fn load_booted(storage: &FlatStorage) -> Result<Vec<Booted>, StorageError> {
    match storage.read_addr(&booted_addr(storage))? {
        Some(bytes) => booted_from_vsf_bytes(&bytes),
        None => Ok(Vec::new()),
    }
}

/// Write the ledger (or delete the entry when it is empty).
pub fn save_booted(list: &[Booted], storage: &FlatStorage) -> Result<(), StorageError> {
    if list.is_empty() {
        return storage.delete_addr(&booted_addr(storage));
    }
    storage.write_addr(&booted_addr(storage), &booted_to_vsf_bytes(list)?)
}

/// Record a boot, keeping the newest stamp per contact. `true` when the ledger changed.
pub fn note_boot(list: &mut Vec<Booted>, entry: Booted) -> bool {
    match list.iter_mut().find(|b| b.handle_proof == entry.handle_proof) {
        Some(b) if b.at >= entry.at => false,
        Some(b) => {
            *b = entry;
            true
        }
        None => {
            list.push(entry);
            true
        }
    }
}

/// Does a live entry stamped `updated` for this contact lose to a recorded boot? A re-add must be strictly newer than the boot to win.
pub fn boot_outranks(list: &[Booted], handle_proof: &[u8; 32], updated: i64) -> bool {
    list.iter().any(|b| b.handle_proof == *handle_proof && b.at >= updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ledger round-trips whole, a newer boot replaces an older one, and only a strictly newer re-add beats a boot.
    #[test]
    fn ledger_round_trips_and_orders_by_stamp() {
        let a = Booted { handle_proof: [1; 32], party_id: [2; 32], at: 1_000 };
        let b = Booted { handle_proof: [3; 32], party_id: [4; 32], at: -5 };
        let bytes = booted_to_vsf_bytes(&[a, b]).unwrap();
        assert_eq!(booted_from_vsf_bytes(&bytes).unwrap(), vec![a, b]);
        let mut list = vec![a];
        assert!(!note_boot(&mut list, Booted { at: 900, ..a }), "an older boot changes nothing");
        assert!(note_boot(&mut list, Booted { at: 1_100, ..a }));
        assert_eq!(list[0].at, 1_100);
        assert!(boot_outranks(&list, &a.handle_proof, 1_100), "a tie keeps the boot");
        assert!(!boot_outranks(&list, &a.handle_proof, 1_101), "a strictly newer re-add wins");
        assert!(!boot_outranks(&list, &[9; 32], 0), "no boot, nothing outranked");
    }
}
