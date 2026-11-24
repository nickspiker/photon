---
name: project-desktop-papers-vault-lost
description: 2026-11-24 Leviathan rebooted onto its retired machine key (papers vault gone); census deleted other roots' rings; fixed 4c0c2eed, cause of the original loss NOT convicted
metadata:
  type: project
---

2026-11-24: after a reboot (11-23 10:38 local), Leviathan's root selection found only the machine-identity vault, ran as the retired key 90e571bf, and the attest asked "add this one?" every time; the fleet member is the papers (TPM) key 469e79b8. The TPM was unchanged (plate c823b5cd), and tohu hadn't changed since 11-09. The papers vault had been deleted at some point before the reboot; the log that would say when lived in /tmp and went with it.

Fixed in photon 4c0c2eed:
- The desktop census keeps the other root's ring, under a pin expiring 2027-02-01. Before, it deleted every ring but the running root's, so any start on the machine key would delete the papers vault.
- At attest, `root_conversion::move_onto_member_papers` carries the machine key's vault into a fresh papers vault, discards the old one through kete, keeps the session, and re-execs on the papers. It reads the held chain first and fgtw.org's copy only when none is held.

Leviathan itself was recovered by hand: the vault was renamed in place and the census deleted the renamed copy ([[feedback-census-deletes-parked-files]]). It now runs on the papers with a fresh vault.

**Why:** the fallback had no way back to a papers key the fleet already lists.
**How to apply:** if another desktop reports "add this one?" after a reboot, check the start log for which root it ran on and for any census lines. The original deletion is still open: watch for a census line naming a `.vsf` ring on a desktop.
