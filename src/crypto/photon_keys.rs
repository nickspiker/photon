//! What Photon's messenger holds of the person: its own alias, never their root (ferros docs/PHOTON.md "Photon's own key: the migration", owner 2026-11-12/13).
//! On ferros the kernel hands the alias in and the identity service the proof and binding; on the desktop and Android Photon's FGTW part derives the alias itself from the person key, with the same function and the same constant for Photon's kist identity, so every platform holds the same bytes.
//! The messenger takes this where it took the session's `identity_seed`; the root stays only in Photon's FGTW part, on the platforms that have one.

/// The person, as Photon's messenger knows them.
#[derive(Clone)]
pub struct PhotonKeys {
    /// Photon's alias seed (`fgtw::alias::seed(person_key, Photon's developer key, Photon's option)`): its signing bundle and its key agreement derive from it, the same on every device of the person's.
    pub alias: [u8; 32],
    /// The person's handle proof (public).
    pub handle_proof: [u8; 32],
    /// The person's pinned identity key, Ed25519 of the identity seed: Photon's identifier for a person stays this key (M2), so no friendship is made again.
    pub identity_pub: [u8; 32],
    /// The binding: "this alias speaks for this handle proof in Photon", signed by a device of the person's fleet with every egg (a VSF document; its typed form is fgtw's once fgtw has it).
    pub binding: Vec<u8>,
    /// The identity seed, on the desktop and Android only, until this device speaks the alias: the old vault addresses, blob names and the old way still derive from it. None on ferros, which never holds the root.
    pub legacy_seed: Option<[u8; 32]>,
}

impl PhotonKeys {
    /// The keys as the platform hands them in: ferros's std/photon from the kernel's alias and the identity service's proof and binding (no legacy seed); the desktop and Android from Photon's FGTW part.
    pub fn new(alias: [u8; 32], handle_proof: [u8; 32], identity_pub: [u8; 32], binding: Vec<u8>, legacy_seed: Option<[u8; 32]>) -> Self {
        PhotonKeys { alias, handle_proof, identity_pub, binding, legacy_seed }
    }
}

impl Drop for PhotonKeys {
    fn drop(&mut self) {
        // The alias and the legacy seed are secrets: wiped when the keys go, as clutch.rs and era.rs wipe theirs.
        use zeroize::Zeroize;
        self.alias.zeroize();
        if let Some(s) = self.legacy_seed.as_mut() {
            s.zeroize();
        }
    }
}
