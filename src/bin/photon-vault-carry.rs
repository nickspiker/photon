//! photon-vault-carry — ONE-SHOT owner tool: copy a pre-cleanup per-identity vault's entries into the new device vault, verbatim.
//!
//! NOT app compat: the app carries no migration layer by doctrine (the census deletes legacy rings on first vault open, docs/fleet-key.md carries keys not files) — this is the OWNER moving his own bytes by hand, on a restored snapshot, BEFORE the new build's first launch gets to sweep.
//! The identity-scope entry derivation is unchanged across the storage cutover, so stored ciphertexts copy raw and decrypt identically — the exact recipe the deleted in-place migration proved with tests.
//! Requires a LIVE tohu session (an attest/resume happened this boot; the tmpfs registers survive a filesystem snapshot restore) — the seeds come from the session, never a typed handle.
//!
//! Order of operations on the hub machine: restore the snapshot (legacy rings return) → run this tool (entries land in the device vault) → launch the new build (census sweeps the legacy remains; the vault already holds everything, including the cached fleet key that lets the carrying establish preserve the fstate slot).
//!
//! Usage: photon-vault-carry [--dry-run]

fn main() {
    let dry = std::env::args().any(|a| a == "--dry-run");
    let session = match tohu::session() {
        Some(s) => s,
        None => {
            eprintln!("carry: no live session in the tohu registers — open photon once this boot (resume or attest), do not reboot, then run this again");
            std::process::exit(2);
        }
    };
    let fp = photon_messenger::network::fgtw::get_machine_fingerprint().expect("carry: machine fingerprint unreadable");
    let kp = photon_messenger::network::fgtw::derive_device_keypair(&fp);
    let secret = *kp.secret.as_bytes();

    use photon_messenger::storage::{App, FlatStorage};
    // The legacy rings live under ONE casing per era (pre-unification "Photon", briefly "photon"). The shared-engine registry keys by FILENAME (identical across casings), so exactly one casing may be opened — take the first that actually has rings on disk.
    let legacy_app = ["Photon", "photon"].into_iter().find_map(|dir| {
        let app = App { id: "photon", dir };
        let paths = kete::vault_ring_paths(app, &session.vault_seed, &secret).ok()?;
        paths.iter().any(|p| p.exists()).then_some((app, paths))
    });
    let Some((legacy_app, paths)) = legacy_app else {
        eprintln!("carry: no legacy per-identity rings found for this session's identity (looked in Photon/ and photon/ under config+data dirs) — nothing to carry");
        std::process::exit(1);
    };
    for p in &paths {
        println!("carry: legacy ring {} ({})", p.display(), if p.exists() { "present" } else { "absent" });
    }
    if dry {
        println!("carry: dry run — nothing copied");
        return;
    }

    let device = FlatStorage::open_device_shared(photon_messenger::storage::APP, secret).expect("carry: device vault open");
    device.set_identity(session.vault_seed).expect("carry: identity bind refused (D2 — this device carries a different identity)");
    let legacy = FlatStorage::open_shared(legacy_app, session.vault_seed, secret).expect("carry: legacy vault open");
    let carried = device.adopt_all_entries_from(&legacy).expect("carry: adopt failed");
    println!("carry: {carried} entr(ies) carried into the device vault — launch photon and let the census sweep the remains");
}
