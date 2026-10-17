---
name: project-offgrid-wfd
description: "Wi-Fi Direct off-grid bearer v1 BUILT 2026-08-30 (Android↔Android) + open-house add; ladder LAN→WAN→WFD→relay; BLE pairing-only; field test + Linux rung pending; docs/offgrid.md"
metadata:
  type: project
---

Canonical: docs/offgrid.md.
Decisions (Nick 2026-08-30): tier ladder LAN → WAN → Wi-Fi Direct → relay last (all ciphertext, so tiering is pure cost); NO BLE for messaging (pairing only); NO hotspotting; iOS out of scope; HARD: never disconnect infra Wi-Fi (STA+P2P concurrency).
v1: no new transport — group members ride the main UDP socket; P2P addresses land in `Contact.p2p_addr` (never local_ip) as `CandidateKind::HostV4P2p`. A per-pair `wfd_cred` (the lower device pubkey is GO + minter) is sealed under the relationship seed; DNS-SD `_photon._udp` carries rotating tokens; the stranded edge = friend unreachable ∧ relay pipe down (`drive_wfd_stranded`).
Open house (add a new friend in the woods): an ephemeral group with SSID+PSK in cleartext TXT (byte-pipe doctrine — trust is CLUTCH); the peer's pt_disc beacon IS the registry record.
Code: src/network/wfd.rs, PhotonWifiDirect.kt + jni_android.rs.

**Why:** friends in range must still reach each other when the internet is gone.
**How to apply:** pending — two-phone field test, the Linux wpa_supplicant D-Bus rung, Windows/mac stubs.
