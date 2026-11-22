---
name: feedback_add_device_confirm_first
description: HARD: add = type handle once, attest, "are you sure?" only when the handle is taken and the device unbound, then a persistent broadcast heard ONLY by fleet devices with their Fleet page open; remove = press remove, type handle, unbind, fleet told at once, old device wipes and nukes Photon (ferros: killswitch)
metadata:
  type: feedback
---

Owner, 2026-11-22. Adding a device (the joiner):
1. Type the handle ONCE on the new device, tap attest, then wait.
2. When it sees the handle is taken and this device is unbound, ask "are you sure?" (the explicit confirmation: "Yes, I really would like to add this device and this is my identity and not a typo").
3. Only if yes: the request is broadcast, persistently, but it "falls on deaf ears unless my fleet page is open on my other devices". No toast, notification or light-up anywhere else: "don't want someone typing Nick in in Germany a bunch of times and making my device light up."
4. Then: put the two devices together (NFC, the other device's screen on and its Fleet page open, in Photon or ferros), else the click to add (BLE touch-select, then the green Add); the words only behind "Not in range? Show words to copy/paste or dictate" (shoulder-surfing).
Removing a device (on the device being removed): press remove once, then type the handle to confirm, then press unbind. The rest of the fleet is notified straight away (no Fleet page need be open). The old device then wipes its vault and keys and Photon is nuked (closed; on Android, just close, no relaunch), or on ferros the killswitch.
Earlier (relayed): he got the fleet notification BEFORE confirming; the auto "not in fleet" join and the re-typed-handle resubmit (launch.rs) both announce without any press.
**Why:** a typed handle (a typo, or a stranger's) must never ring a person's devices; words on screen can be read over a shoulder; a removal is the owner's own act and must be unmistakable.
**How to apply:** the flows and their words live in fgtw (FLOWS.md: a Join machine and the departure), per [[feedback_fgtw_out_of_photon]]; Photon's screens and ferros's Fleet page execute them; release is never blocked by unsynced rows, only warned ([[project_device_removal_safety]]).
**Built 2026-11-22** (fgtw e5766f4, FLOWS.md "Join and Remove"): `fgtw::flow::join::Join` (Looking → Confirm → Asking only on the yes; fgtw.org only on `Wake::Unheard`, 4¼ s with no fleet device heard on this network, or `ShowWords`; words only after `ShowWords`), the sponsor's `AsksToJoin` only with its Fleet page open, and Depart's `Removal` (Remove → typed handle's proof → Unbind; a bare Leave refused; `hold` for "Copy, then release"). Photon executes them in `ui/photon_app/join.rs` and the Security page's Release (handle box + Unbind). The removal stays bilateral (a member approves) and the leaver wipes on de-fold; the owner's "fleet told at once" is the depart_req's delivery. Open: a join over BLE or NFC with no shared network; ferros's adoption.
