# Off-grid bearers — messages in the woods, no infrastructure, no pairing, no user intervention

## The insight

Link-layer pairing exists to bootstrap trust between strangers; photon already carries its own trust (CLUTCH, sealed frames, keyed tokens).
So every radio below is used with pre-shared or zero link-layer trust — the radio is just a byte pipe, and the sealed frames ride it unchanged.
iOS is OUT OF SCOPE by decision (2026-08-30): sideloading requires a tethered developer identity, and this app doesn't do gatekeepers.
BLE is DEMOTED to device pairing only (decision 2026-08-30): the friend channel pre-provisions everything a Wi-Fi Direct meetup needs, so no bootstrap radio is required.

## The tier ladder (decision 2026-08-30)

All traffic is ciphertext end-to-end, so tiering is pure cost/availability, never security:
1. LAN — same network, existing multicast discovery + direct UDP. Cheapest, full throughput.
2. WAN — punched direct path (the traverse candidate machinery).
3. Wi-Fi Direct — infrastructureless group when no shared network exists.
4. Relay — last resort, as today.
HARD CONSTRAINT: bringing up any off-grid bearer must NEVER disconnect the device's infrastructure WiFi. Android's STA+P2P concurrency carries this; the GO uses BAND_AUTO so the framework co-channels with the STA link.

## Wi-Fi Direct design (BUILT 2026-08-30, v1 slice: Android↔Android)

### No new transport
Once a P2P group forms, both sides hold real IPs on the p2p interface (GO 192.168.49.1, client DHCP'd), so frames ride the EXISTING wildcard-bound UDP socket — no select! arm, no sentinel, no inject channel.
The bearer is only: discovery, group bring-up, address registration.
Multicast on the p2p iface is OEM-flaky and unnecessary: after group-up the joiner sends the existing pt_disc beacon UNICAST to the GO (LanBroadcastRequest::unicast); the GO learns the joiner from the frame source and beacons back on the learn edge (self-quenching).
The 192.168.49.x address lands in `Contact.p2p_addr` (never `local_ip` — it must not hit the foreign-/24 gate or survive teardown), enters candidate gathering as `HostV4P2p` (priority 50: below infra LAN 60, above punched WAN 40), and normal punch validation adopts the path.

### Credentials: pre-provisioned per-pair over the normal sealed channel
`wfd_cred` record per friend pair: `go` (designated group owner = lexicographically-LOWER device pubkey — deterministic, zero negotiation), `ssid` (DIRECT-ph-…), `psk`, `epoch` (monotonic, newer replaces older).
Minted by the elected-GO side on the friend's came-online edge (once per session — a lost frame self-heals next session; receiver adopts idempotently by epoch), sealed under `wfd_seal_key = keyed_hash(relationship_seed, domain)`, carried in a signed `wfd_cred` frame, persisted in contact state.
So an offline meetup needs ZERO bootstrap radio — both phones already hold the group credentials.
Connect is dialog-free both directions on API 29+ (pre-shared WifiP2pConfig skips WPS).

### Discovery: DNS-SD service frames, no BLE
`_photon._udp` local service; the TXT record carries rotating friend-recognizable tokens: `keyed_hash(pairwise_token_key, device_pubkey ‖ coarse_hour)` truncated to 16B, one per provisioned friend, hourly rotation (+previous hour matched for skew) so strangers can't track; instance name random per boot.
Fully dialog-free; NEARBY_WIFI_DEVICES (33+) / fine-location (older) runtime permission with the pending-grant re-run pattern.

### Open house — adding a NEW friend in the woods (BUILT 2026-08-30)
The per-pair tokens/credentials only serve EXISTING friends; two strangers meeting off-grid have neither.
The magic ID insight: the `_photon._udp` service type already is the universal marker — so the add flow uses it directly.
Submitting an add while the registry is unreachable (relay-pipe-down proxy) arms OPEN HOUSE: mint an ephemeral group, createGroup, and publish the SSID+PSK IN THE CLEAR in the TXT record (`ss`/`pk` keys).
Cleartext creds are coffee-shop-WiFi exposure — the group is a byte pipe, trust is CLUTCH, a hostile joiner sees only ciphertext and unresolvable knocks — and the trackable "photon user here" beacon exists only during the deliberate add flow.
Both people add each other (mutual consent is required anyway); when both open houses hear each other, the lexicographically-lower SSID stays GO and the other tears down and joins (deterministic, symmetric).
The typed handle's proof derives locally (~1s memory-hard PoW, off-thread); the peer's pt_disc beacon matching that proof IS the registry record — hp in the provenance, device key in `ke`, address in the source — so the contact is created from the beacon and ping → pong → CLUTCH ride the group.
On find: the cleartext beacon quiets (stop_open_house keeps the group for the ceremony); leaving the add flow with nobody found disarms everything, group included.
The joiner also unicasts its pt_disc at the GO each ping cadence, so a contact added AFTER group-up still gets announced.

### Edges, not timers
- STRANDED-ENTER (evaluated on the ping cadence): a provisioned friend has no validated path and is offline AND the relay pipe is down (`wfd::RELAY_REACHABLE`, set by the pipe task's connect/liveness transitions) → start advertise + discovery.
- FRIEND-HEARD: a TXT token matches a provisioned friend with no path → the credential's GO creates the group, the other side connects.
- GROUP-UP: platform up-call → unicast pt_disc exchange → punch validation → normal traffic.
- STRANDED-EXIT: paths recover or the relay returns → stop advertise + discovery.
- DRAINED: group up but every p2p peer went silent (failed-ping hysteresis) → removeGroup.
- IFACE-LOST: platform reports the group gone → clear every `p2p_addr` + any validated path inside 192.168.49/24 (no black-holing).

### The pieces
- src/network/wfd.rs — tokens, credential mint/seal/open, GO tie-break, bearer state machine (Idle→Stranded→Forming→Up), platform trait, event queue, relay-reachability flag.
- src/network/traverse/candidate.rs `HostV4P2p`; gather.rs feeds `Contact.p2p_addr` + exempts the WFD subnet from the foreign-LAN filter.
- src/ui/photon_app/status.rs — event→StatusUpdate conversion, WfdCredReceived/WfdFriendNearby/WfdGroupUp/WfdGroupDown arms, LanPeerDiscovered p2p routing; sync.rs — provisioning + stranded driver.
- android PhotonWifiDirect.kt + jni_android.rs bridge (PhotonBeacon lifecycle pattern).

## Later rungs

- Linux: wpa_supplicant P2PDevice D-Bus (zbus), feature-gated `wfd-linux`, NM-unmanaged p2p ifaces — implements the same WfdPlatform trait. The Android↔laptop rung.
- Windows: WinRT WiFiDirect; macOS: no public P2P — Multipeer covers mac↔mac someday. Stubs (NullWfd) until then.
- Wi-Fi Aware (NAN) as an alternative Android datapath if Direct proves flaky in the field.
- LoRa for miles-not-meters: 2-15km at bytes/s, texts only, a hardware-dongle story that fits the PIPE direction.

## Field verification (pending)

- Two phones on the same AP: WFD never engages (LAN wins), infra WiFi never drops.
- Woods test (no AP): STRANDED fires, DNS-SD hears the friend, dialog-free group forms, chat round-trips, PathValidated logs a 192.168.49.x remote.
- Concurrency: one phone on infra keeps internet while the group runs; group frequency matches the STA channel.
- Teardown: walk out of range → hysteresis → removeGroup; return → rediscovery.
