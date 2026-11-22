---
name: feedback_add_device_confirm_first
description: HARD: adding a device announces nothing until the new device's person confirms; then NFC / BLE touch-select; words only behind "Not in range?"
metadata:
  type: feedback
---

Owner, 2026-11-22 (relayed by the ferros coordinator; ferros memory fleet_add_needs_open.md): he got the fleet notification on his devices BEFORE pressing the confirmation on the new device.
1. Nothing reaches the person's other devices until the new device's person presses the explicit confirmation ("Yes, I really would like to add this device and this is my identity and not a typo"). Never announce on a typed handle alone.
2. After that, the new device advises: put the two devices together (NFC), the other device's screen on and its Fleet page open (Photon or ferros); otherwise the click to add (BLE touch-select, then the green Add).
3. The words are for odd cases only, never shown by default (shoulder-surfing): behind "Not in range? Show words to copy/paste or dictate".
**Why:** a typed handle (a typo, or someone else's) must not ring a person's devices, and the words shown on screen can be read over a shoulder.
**How to apply:** the flow and its wording live in fgtw (FLOWS.md), per [[feedback_fgtw_out_of_photon]]; Photon's Add screen and ferros's Fleet page follow it. Related: [[project_pairing_v2]], [[project_lifecycle_flows]].
