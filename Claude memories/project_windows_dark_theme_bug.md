---
name: project-windows-dark-theme-bug
description: "PINNED: photon install on friend-J's Windows box corrupted the OS dark-theme Start-menu/search text (000000→FFFFFF on F0F0F0); toggling Windows light/dark fixed it; investigate later"
metadata: 
  node_type: memory
  type: project
  originSessionId: bf3c2e39-d57b-4469-8848-1780b1b5c927
---

Reported 2026-07-24 by friend-J: right after installing photon on Windows, the WINDOWS OS dark-mode search-results/Start-menu text went unreadable (was 000000 on F0F0F0, became FFFFFF).
Reboot and display-settings reset did NOT fix it; toggling Windows colour scheme dark→light→dark DID (classic shell theme-cache corruption signature).
Only the dark-mode search results display was affected; unknown whether relaunch re-triggers.
We intentionally touch NO Windows compositing/theme state; suspects to audit when picked up: winit's DWMWA_USE_IMMERSIVE_DARK_MODE per-window attribute, the UpdateLayeredWindow path (windows_layered.rs), anything the install-*.ps1 installer writes, and any SystemParametersInfo use with persist flags.
Related ask (queued feature): Security screen gains a full WIPE/UNINSTALL (nuke vault + remove the install itself) partly to bisect issues like this.

Related: [[project-fgtw-migration-state]].
