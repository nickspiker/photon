# Off-grid bearers — messages in the woods, no infrastructure, no pairing, no user intervention

## The insight

Link-layer pairing exists to bootstrap trust between strangers; photon already carries its own trust (CLUTCH, sealed frames, handle_proof beacons).
So every radio below is used UNAUTHENTICATED at the link layer — the radio is just a byte pipe, and the sealed VSF frames ride it unchanged.
Architecturally each bearer is one new injection arm into the existing receive pipeline, the exact precedent the relay pipe set (frames injected into the select! as datagrams tagged with a sentinel source).
The durable re-serve machinery makes every bearer delay-tolerant for free: undelivered rows flow whenever a peer walks into range.
iOS is OUT OF SCOPE by decision (2026-08-30): sideloading requires a tethered developer identity, and this app doesn't do gatekeepers.

## The tier ladder (per-pair capability negotiation, same shape as the punch tiers)

1. BLE coded advertisements — the universal discovery floor + last-resort data path.
2. Best mutual WiFi rung for the data path: Wi-Fi Aware > BLE-bootstrapped hotspot > platform-specific (below).
3. A Linux laptop as the long-range node: raw 802.11 tricks phones can't do.

## BLE (all platforms)

- Pairing is optional in BLE: extended advertisements (~255B connectionless — enough for the LAN-discovery beacon shape: handle_proof + device key) and unauthenticated GATT connections.
- Range: 1M PHY ~10-30m indoors / ~100m open; LE Coded PHY (S=8) 125kbps nominal, 300m-1km+ line of sight — texts at ~10-40kbps effective, a chat frame is nothing, a 573KB offer is minutes.
- Coded PHY support: common on recent Android; desktop radios vary (BlueZ exposes it on Linux where the chip does; WinRT likewise; macOS CoreBluetooth does not expose PHY selection).

## WiFi per platform, zero-intervention tricks

- Android: Wi-Fi Aware (NAN) — programmatic discovery + direct encrypted datapath, no AP, no dialog. Fallback: LocalOnlyHotspot (app-created, random SSID/PSK) with the credentials carried over the unpaired BLE channel sealed to the friend's keys, peer auto-joins via WifiNetworkSpecifier.
- Windows: no public Wi-Fi Aware. WinRT WiFiDirect API for direct links; Mobile-Hotspot via NetworkOperatorTetheringManager + BLE-carried credentials as the fallback; BLE GATT always.
- macOS: no public Aware/AWDL access; Multipeer Connectivity covers mac↔mac. Otherwise CoreWLAN can JOIN a network programmatically (location permission), so the BLE-bootstrapped hotspot works with a mac as the JOINER; macs can't raise a hotspot programmatically.
- Linux: everything — 802.11s mesh mode (a real multi-hop mesh between laptops), IBSS ad-hoc, hostapd-raised AP + BLE bootstrap, and monitor-mode injection for the truly connectionless km-class directional-antenna case. The backpack relay node.
- Redox: whatever the driver story becomes; the bearer abstraction keeps it a later arm, not a redesign.

## The endgame for miles, not meters

LoRa: 2-15km at bytes-per-second on unlicensed spectrum, texts only, proven by Meshtastic — a hardware dongle story that fits the PIPE direction, not a phone-radio one.

## Status

Design only — nothing built. First build slice: BLE extended-advertisement discovery beacons (the existing LAN beacon shape over a new radio) + unauthenticated GATT frame exchange, Android + Linux first; then the Aware datapath; then the BLE→hotspot bootstrap for Windows/macOS coverage.
