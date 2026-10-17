---
name: project_fleet_holds_everything
description: "DOCTRINE (Nick 2026-10-07): every fleet device holds a copy of EVERY content category — photons, waves, pigeons, beams; pull one device at a time, easiest first; sane on metered; blob replicate_sweep v1 BUILT 2026-10-07"
metadata:
  type: project
---

Nick 2026-10-07: *"if I have five devices, all five should get copies of waves, pigeons, beams, photons, all of it… in flight even if bandwidth allows but be sane."*

**Why:** FLEET-HOLDS-HISTORY (docs/durability.md) — any device may be asleep or gone; a copy on one device is not durability. Found via Jeff's PDF ([[project_pigeon_fetch_reach]]): pigeons reached only the friend's in-hand device.
**How to apply:** (1) siblings pull every new blob row when it syncs, in flight where bandwidth allows; (2) a device lacking bytes pulls from ONE holder at a time, easiest first (online + validated + LAN) — never every holder pushing at once ([[project_pigeon_fetch_fanout]]); (3) metered defers, storage obeys per-category retention ([[project_retention_winnow]]). TICKETS "THE FLEET HOLDS EVERYTHING" is the build entry.

**BUILT v1 (2026-10-07):** `replicate_sweep` in attachments.rs — edge-driven (`replicate_dirty`: row count moved, presence probes landed, metered→unmetered); arms each file row with `blob_present_known == Some(false)` into `attach_fetch_inflight` once per session; `attach_fetch_retry_tick` asks and rotates devices; skips metered networks and wave recordings when `waves.hold` is off. Meta line says "on this device" / "not on this device".
Also: PDF page-one previews via hayro (send-time preview blob for new PDFs, local render from the original for old rows). Photons replicate via chain sync; wave recordings under `wave_hold`; beams still unreplicated. Residue in TICKETS.
