//! Live end-to-end attest smoke against production fgtw.org — run explicitly with:
//!   cargo test --features development --test live_attest_smoke -- --ignored
//! Exercises the full verified-read stack in one flow: challenge (read_verified pinned to the FGTW key) → fleet fetch (not_found error frame recognized) → genesis publish (+hb chain) → verified read-back → announce → encrypted_peers response thru parse_document.
//! Leaves a throwaway fleet chain + peer entry on the server (test infra; nuke at will).

use photon_messenger::network::fgtw::bootstrap::load_bootstrap_peers;
use photon_messenger::network::fgtw::Keypair;
use photon_messenger::types::Handle;

#[test]
#[ignore]
fn live_attest_smoke() {
    // Unique-ish throwaway handle so reruns don't collide with a stale chain from a prior run's device key.
    let handle = format!("smoketest{}", std::process::id());
    let identity_seed = photon_messenger::storage::contacts::derive_identity_seed(&handle);
    let handle_proof = Handle::username_to_handle_proof(&handle); // ~1s memory-hard proof

    // Deterministic throwaway device key (NOT this machine's real identity).
    let secret = ed25519_dalek::SigningKey::from_bytes(&[0xA7; 32]);
    let device_key = Keypair {
        public: secret.verifying_key(),
        secret,
    };

    let result = photon_messenger::network::http::runtime().block_on(load_bootstrap_peers(
        &device_key,
        handle_proof,
        5546,
        &identity_seed,
    ));

    assert!(
        result.error.is_none(),
        "attest flow failed: {}",
        result.error.unwrap()
    );
    println!(
        "attest OK for {} — {} peer(s) returned",
        handle,
        result.peers.len()
    );
}
