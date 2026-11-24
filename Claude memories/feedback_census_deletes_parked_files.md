---
name: feedback-census-deletes-parked-files
description: HARD — never park or rename a file inside Photon's app dirs (~/.local/share/photon, ~/.config/photon, Android files/photon): the storage census deletes everything there that is not the vault ring or the log
metadata:
  type: feedback
---

2026-11-24: to move Leviathan off its old machine-key vault, I renamed `SfbR….vsf` to `SfbR….vsf.machine-key-90e571bf-2026-11-24` in place, in both ~/.local/share/photon and ~/.config/photon, and relaunched Photon. At startup the census logged "STORAGE: census — … deleted (the census is the vault ring + the log, nothing else)" and removed both copies. The only remaining copy is in the hourly /home btrfs snapshots (root only).

**Why:** the census is the owner's rule that Photon's dirs hold exactly the vault ring and the log (Rule -1: no loose files), so it deletes anything else, renamed backups included.
**How to apply:** to set a vault aside, MOVE it out of every Photon app dir (e.g. to ~/photon-vault-parked-<date>/) before Photon starts, or ask the owner; never rename it in place. The same goes for any file you want Photon not to touch.
