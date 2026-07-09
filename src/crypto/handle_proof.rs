//! Handle attestation proof — re-exports `ihi::handle_proof`, the memory-hard one-way function (~1s) that turns a handle's hash into its anti-squat attestation proof.
//! Squatting a handle costs real compute per attempt, so the namespace can't be swept cheaply; the derivation itself is the PIPE-silicon-exact `ihi` primitive (see the `ihi` crate's memory-hard PoW, distinct from its lossy chaos_amp OWF).

pub use ihi::handle_proof;
