---
name: project_fleet_holds_everything
description: "DOCTRINE (Nick 2026-10-07): every fleet device holds a copy of EVERY content category — photons, waves, pigeons, beams — replicated in flight where bandwidth allows, sane on metered/storage; receiver pulls one device at a time, easiest first; unbuilt for blobs"
metadata:
  type: project
---

Nick 2026-10-07: *"if I have five devices, all five should get copies of waves, pigeons, beams, photons, all of it… in flight even if bandwidth allows but be sane."*

**State (2026-10-07):** photons replicate (chain sync, fleet braid plane). Wave RECORDINGS replicate to siblings under `wave_hold`. PIGEONS do not: pushed once to the friend's in-hand device, siblings never get a copy (send code says "siblings fetch on demand" from a fleet store that does not exist for blobs), receiver never auto-fetches documents. Beams are unbuilt. Found via Jeff's PDF ([[project_pigeon_fetch_reach]]).

**Why:** FLEET-HOLDS-HISTORY — any device may be asleep or gone; a copy on one device is not durability.

**How to apply:** (1) siblings pull every new blob row when it syncs, in flight while live where bandwidth allows; (2) a friend's device lacking bytes pulls from ONE of the sender's devices at a time, easiest first (online + validated + LAN) — never every holder pushing at once (the September duplicate-upload class, [[project_pigeon_fetch_fanout]]); (3) sane: metered defers, storage obeys the per-category retention horizons ([[project_retention_winnow]]). TICKETS "THE FLEET HOLDS EVERYTHING" is the build entry.
