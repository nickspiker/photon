---
name: project_group_pigeon_stuck_fetch
description: 2026-11-01 group pigeon (Emma's group-wave recording bed894fe) never reached Nick's desktop — fetch aimed at Emma's offline pinned device never ended; FIXED device_online + rank use the device's own ep.online
metadata:
  type: project
---
Field 2026-11-01 (Nick desktop, Emma, Brittany): Brittany's jpeg reached everyone online (push to one device per member, the rest fetched); Emma's group-wave recording bed894fe never reached Nick's desktop in 4 h.
Convicted: the desktop asked Emma's pinned device 8b29d719 (offline, its address answered by her other devices: "pong answered by 0a08c6e4 but we pinged 8b29d719" ×68). attachments.rs device_online returned the CONTACT's is_online for the pinned device — the person, kept true by her other devices — so fetch_quiet never fired despite 34 per-device timeouts; fetch_pong_from counts only the asked device's pongs; the fetch is armed once per session, so it sat until restart. Nick's own MacBook held the file and was never asked.
FIXED: device_online reads the device's own endpoint verdict first (ep.online, set on every pong/timeout in status.rs), person-level only as fallback; the ask ranking uses it for the pinned device too.
Open: a "pong answered by X but we pinged Y" (network/status.rs:3602) could end asks aimed at Y — needs a StatusUpdate; the pinned-device address on Nick's desktop still points at Emma's other devices; Brittany got the row itself only at 02:05 (era drops "frame tagged era … from 9ce64cc2 … dropped" — the open [[project_emma_brittany_group_fork]]).

**Why:** a person-level flag standing in for a device verdict leaves asks parked forever on a dead device.
**How to apply:** per-device decisions read DeviceEndpoint.online; Contact.is_online is the person (any device up). Related: [[project_address_sets]].
