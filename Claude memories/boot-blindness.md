---
name: boot-blindness
description: "2026-08-12 OPEN: boot never reads fleet_settings from disk (zoom waits for first network merge) + scoped-blob contact avatars are never cached locally (every boot re-fetches)"
metadata: 
  node_type: memory
  type: project
  originSessionId: 81588914-5914-4600-bb98-72cc4fae2260
  modified: 2026-08-13T05:50:49.961Z
---

FIELD-VERIFIED 2026-08-12 21:03 fresh launch (v0.51.191): zoom restored BEFORE Ready ✓, own avatar +128ms async ✓, all 9 peer avatars installed ~180ms post-Ready from local vault (no network) ✓, keypairs 0ms (0 rehydrated — was the old bulk) ✓. PERF line: "vault 496ms, contacts 278ms, migrations 8ms, messages 141ms, chains 6ms, keypairs 0ms, settings 1ms → local Ready in 937ms". REMAINING EATERS (all kete storage, ~915 of 937ms): kete open_shared 496ms, contact-state loads 278ms (~23ms per read_addr?! per-read overhead suspect — KDF or index walk per key), messages 141ms. Next dig = the kete crate (~/Code/kete): why open costs 500ms and why per-read costs ~20ms. Goal remains local paint < 16ms.

2026-08-13 second symptom, same root: MacBook window-drag lag = check_status_updates 88-559ms passes — fleet_key_cached() did a vault read_addr PER CALL (once per inbound history page, UI thread), each stalling behind the async writers' kete commits during backfill storms. FIXED photon 4417b90: fleet key in RAM (Arc<Mutex>, refreshed by every key-writer thread, cleared at logout). Any remaining big status passes after this build point back at kete per-op cost/lock contention — same dig. Also note: save/load_clutch_keypairs are memory-only NO-OPS (nothing persisted), so the boot "rehydrate" loop never loads anything regardless of filter.

Nick's report 2026-08-12: opening Photon shows default scale and no avatars until the network answers, though both are on disk. Two verified causes (both now fixed, above):

1. ZOOM/SCALE: `display.zoom` persists locally in fleet_settings, but NOTHING loads fleet_settings at boot — every `ensure_fleet_settings()` caller is a user action (You page, hardlogs toggle, zoom persist) or the fleet-merge network drain (photon_app.rs ~10818). First network merge → `apply_settings_to_ui` → zoom restore. Offline boot = default zoom forever. Fix: call `self.ensure_fleet_settings()` in init's resume arm right after `self.storage = Some(s)` (~photon_app.rs:3132) — disk-only, idempotent, the `zoom_restored` one-shot latch already guards re-application.

2. CONTACT AVATARS: the scoped-blob path (current scheme, avatar_scoped::fetch_blocking in the spawn at ~photon_app.rs:16112) ALWAYS hits the network and NEVER caches; only the legacy pin fallback (`download_avatar_pinned`) is cache-first + cache-after-fetch ("party-id scope, so a restart shows it without a round-trip"). Fix shape: cache the scoped fetch's AV1 under a party-id vault key and read it before the network; note scoped raw is plain AV1 while the pin cache stores the VSF envelope — distinct key or unified re-wrap.

OWN avatar is fine: init loads it from the vault the moment storage opens (~3135); only a cleared vault waits on FGTW recovery. Contacts/messages also load from disk in init (census proves) — the visible "nothing loads" is zoom + avatars. Related: [[self-pair-sibling-row]].
