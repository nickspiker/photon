---
name: project-add-device-lan-path
description: 2026-11-22 adding a device over the LAN WORKED for the first time (8a → Brittany); the four faults that had broken every add avenue, and the 3a fleet-key split
metadata:
  type: project
---

Owner 2026-11-22: "Worked." The 8a joined Brittany's fleet over the LAN on photon 36311321 (8a, adb install) with her phone on dev v0.155.1.

Every add avenue (NFC tap, BLE pick, words, "Not in range?") waits on the joiner's binding request reaching a sponsor. Four faults stood in the way:
- The joiner never processed ANY network update: `check_status_updates` returned when `own_party_id()` was None (every unattested device, since at least 08-14). Fixed 36311321 with `drain_joining_updates` (join.rs). The LAN join path (10-26) had therefore never worked; joins only ever finished thru fgtw.org's registry.
- The request (~10 KB with SPHINCS+) rides PT, and PT opens a transfer only for a contact (NET-01). Fixed d3443513 with `admit_lan_device` (a vouched list, bounded to 1<<3 entries) plus a unicast beacon before each hand.
- "Local first" (11-10) stopped the default registry post, so nothing carried the request.
- The fgtw.org worker refused every request carrying a plate kind ("pk"), matching `VsfType::u(k,false)` exactly when an auto-sized u decodes as u3/u4 (see [[feedback-vsf-readers-width-agnostic]]). Fixed fgtw-bootstrap fb72dee and deployed.

Android log: files are created 0660 (desktop 0600). Android 17's external storage ignores chmod, so an old 0600 log is rewritten once in place. adb pull works again, which lets a device that can't submit (a joiner) still be read.

**3a fleet-key split (same day):** the owner's Pixel 3a (fd1fa6a7) was added by the Pro (op 29). Published fan-out rev 2762 key 706917fe (publisher fd1fa6a7, 5 wraps): the 3a ran the shrink heal because the fan-out still wrapped the locked 1be949c1. Leviathan and the Pro never adopted it. Leviathan waited on "a rotation in flight" and then re-sealed fstate under its own old key. The Pro only checked the fan-out when it held no key. Fixed d9126bd4 on the Photon side (sync from the fan-out on a sibling-state failure; re-seal only when our key is the published one). ferros fixed the Pro side (forge 70ea52ab), and the Pro adopted 706917fe. Leviathan picks the key up on its next reload. f54938f7: FSTATE lines had logged the fleet key's own first 4 bytes and now log fgtw's derived fingerprint, so old logs expose 32 bits of the retired key 31dded93.

**Why:** "used to work" = the registry path. Two changes broke it at once, and the LAN path that replaced it was never live.
**How to apply:** a join stall → read BOTH logs: sponsor (photonlog --pull), joiner (adb pull of photon.log.vsf). `pid-probe --hp <hp>` prints the published fan-out with no handle needed.
