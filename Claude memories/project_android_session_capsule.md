---
name: project_android_session_capsule
description: "Android de-attest-on-restart FIXED — boot-locked session capsule built in tohu (docs/android-session-persistence.md); boot secret falls back to Settings.Global.BOOT_COUNT where boot_id is blocked"
metadata:
  type: project
---

**Bug:** Android de-attested on a plain app restart; the only persistence was a sticky broadcast, which dies with the app on Samsung (tested 2026-07-06).

**Fix** (design: docs/android-session-persistence.md; built in tohu, `session_capsule_paths`): the 96-byte session roots (`identity_seed ‖ vault_seed ‖ handle_proof`) sealed in a VSF capsule under a per-boot wairua = `ihi::spaghettify(boot_id)`, written to several local tiers; the first tier that opens wins, all fail → attest.
Power is the boundary: same boot → seamless restart; reboot → re-type the handle.
A capsule miss with session dirs set is FINAL (never fall back to the tmp path; test-pinned).

**2026-07-25 (Sarah, Samsung):** ROMs whose SELinux blocks `/proc/sys/kernel/random/boot_id` never persisted, because `tohu::set_boot_secret_override` existed but was unwired.
Fixed c40a3f2: Kotlin derives the boot secret from `Settings.Global.BOOT_COUNT` (pre-API-24: per-install random prefs) and hands it over in `nativeNetworkInit` before session wiring.

**How to apply:** a recurring "not attested after restart" on one ROM → check the wairua source first. No password ever: the device is the credential, the handle a public username. See [[project_session_registers]], [[project_secret_memory_hygiene]].

**2026-10-21 (Jeff, re-attest with no reboot):** BOOT_COUNT is NOT per-power-cycle — Android bumps it on a userspace restart (framework restart, e.g. a Play system update) while the kernel keeps running (log: bootcount 350 → 351, uptime 13d 6h → 14d 0h). Since c40a3f2 Kotlin set the count as the override on every phone, so the capsule died without a reboot. Fixed photon 1st commit after v122 + tohu 9f9b254: Kotlin reads /proc/sys/kernel/random/boot_id first (count = fallback where SELinux blocks it), and passes the count as tohu's LEGACY secret — `session()` opens a capsule with it once and re-seals under boot_id, so the switch costs no re-attest. Samsung-blocked ROMs stay on the count (soft-restart weakness remains there).
