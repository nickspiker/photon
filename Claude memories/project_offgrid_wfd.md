---
name: project-offgrid-wfd
description: Wi-Fi Direct off-grid bearer v1 BUILT 2026-08-30 (Android↔Android); tier ladder LAN→WAN→WFD→relay decided; BLE demoted to pairing-only; field test + Linux rung pending
metadata: 
  node_type: memory
  type: project
  originSessionId: 9486060b-4dcb-4d7e-be00-c2a128f2d9f5
---

Off-grid decisions (2026-08-30, Nick): tier ladder LAN → WAN → Wi-Fi Direct → relay-last (all ciphertext so tiering is pure cost); NO BLE for messaging (pairing only); NO hotspotting; iOS out of scope; HARD constraint = never disconnect infra WiFi (STA+P2P concurrency, GO uses BAND_AUTO).
v1 BUILT (docs/offgrid.md is authoritative): no new transport — group members ride the main wildcard UDP socket; joiner sends pt_disc UNICAST to GO (LanBroadcastRequest::unicast), GO answers on the learn edge (self-quenching); 192.168.49.x lands in Contact.p2p_addr (NEVER local_ip) as CandidateKind::HostV4P2p priority 50.
Credentials: per-pair `wfd_cred` (go=lower-device-pubkey elected GO+minter, ssid/psk/epoch) minted on the friend's came-online edge once per session, sealed under keyed_hash(relationship_seed, wfd domains), persisted in contact state (wfd_go/ssid/psk/epoch fields); receiver adopts iff epoch newer.
Discovery: DNS-SD `_photon._udp` TXT = rotating 16B tokens keyed_hash(pair, device‖coarse_hour); stranded edge = provisioned friend unreachable ∧ relay pipe down (wfd::RELAY_REACHABLE set by pipe task), evaluated on the ping cadence (drive_wfd_stranded in sync.rs).
Pieces: src/network/wfd.rs (bearer singleton + event queue + platform trait), PhotonWifiDirect.kt + jni_android.rs bridge (PhotonBeacon pattern), drain arms in ui status.rs, vsf-gate baseline wfd.rs=1 (post-AEAD inner parse class).
OPEN HOUSE (new-friend-in-the-woods, BUILT same day): submit add while relay down → arm open house (ephemeral group, SSID+PSK CLEARTEXT in TXT ss/pk keys — byte-pipe doctrine, trust is CLUTCH); handle_proof derived locally off-thread (~1s PoW); the peer's pt_disc beacon IS the registry record (hp+ke+source) → contact created from it; dual-open-house tie-break = lower SSID stays GO; found → beacon quiets, group stays for ceremony; leaving add flow disarms all.
PENDING: two-phone field test (friend tier + open-house add), [[project-peers-are-fgtw]] Linux wpa_supplicant D-Bus rung (`wfd-linux`), Windows/mac stubs, LoRa horizon.
