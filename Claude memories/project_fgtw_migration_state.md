---
name: project_fgtw_migration_state
description: fgtw-crate substrate migration is done through M3 (keys/fleet/fanout/fstate/pair/client); protocol split + DHT/blob deferred
metadata: 
  node_type: memory
  type: project
  originSessionId: 0b164fd9-062c-407e-b4bb-8f6be8d1982d
---

The bottom-up extraction of the FGTW substrate from `photon/src/network/fgtw/` into the `fgtw` crate is **done through M3** (2026-07-04), paused by decision after that:

- **M1** `fgtw::keys` (Keypair) + `fgtw::fleet` (membership chain: fold/verify + VSF op codec + builders). The FGTW worker (`fgtw-bootstrap`) deleted its hand-mirrored `fleet.rs` → `pub use fgtw::fleet::*`.
- **M2** `fgtw::fanout` (fan-out seal/open), `fgtw::fstate` (roster codec), `fgtw::pair` (pairing words) — behind the `fanout` feature (optional deps x25519/chacha/rand/voca/num-bigint, off the worker's base surface).
- **M3** `fgtw::client` — the whole fetch-then-sign oracle, transport-agnostic via the `FgtwTransport` (post bytes → {status, body}) + `FleetSealer` (roster AEAD) traits. Photon supplies `PhotonTransport` (pooled reqwest + short errors) + `PhotonSealer` (kete).

**How photon rides it:** `network/fgtw/fleet.rs` is now a thin binding — re-exports the crate types + same-signature wrapper fns injecting the transport — so every `crate::network::fgtw::fleet::*` call site is unchanged. Wire format never changed; a mid-migration photon still talks to the deployed worker.

**Deferred (see fgtw/MIGRATION.md):**
- **M4 protocol.rs split** — `FgtwMessage` is ONE enum mixing DHT + generic-FGTW + photon-messaging (chat/status/avatar) variants; splitting it is an architecture call (two enums+tag / whole-enum-to-crate / generic + opaque `App(bytes)`) that sits on the messaging boundary. Do it AS PART of the messaging rework, not before.
- **M5 blob + DHT (node/peer_store/relay) + fingerprint/tohu reconcile** — clean generic moves, but wait until a consumer (calendar) actually needs them.

**Why:** [[project_fgtw_nostd_deferred]] (crate is std for now). The migration's whole point was to extract the substrate BEFORE reworking messaging so messaging lands on a clean crate boundary — that's now achieved; the messaging rework is the next piece of work.
