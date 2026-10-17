---
name: project_fgtw_migration_state
description: fgtw-crate substrate migration done through M3 (keys/fleet/fanout/fstate/pair/client, 2026-07-04); M4 protocol split + M5 blob/DHT deferred (fgtw/MIGRATION.md)
metadata:
  type: project
---

The FGTW substrate moved from `photon/src/network/fgtw/` into the `fgtw` crate thru M3 (2026-07-04): M1 `keys` + `fleet` (the worker uses `pub use fgtw::fleet::*`), M2 `fanout` / `fstate` / `pair` behind the `fanout` feature, M3 `client` (transport-agnostic via `FgtwTransport` + `FleetSealer`; photon supplies `PhotonTransport` + `PhotonSealer`).
Photon's `network/fgtw/fleet.rs` is a thin binding that re-exports and injects the transport, so call sites never changed.

**Deferred (fgtw/MIGRATION.md):**
- M4 `protocol.rs` split — `FgtwMessage` mixes DHT, generic FGTW and photon messaging; split it AS PART of the messaging rework, not before.
- M5 blob + DHT + fingerprint/tohu reconcile — wait for a consumer that needs them.
The crate stays std: [[project_fgtw_nostd_deferred]].
