---
name: project_vault_seal_failures
description: "CONVICTED + FIXES SHIPPED 2026-09-04: Leviathan's 43 danglings = live-lap (appends over referenced blocks); lap guard + airlock doubling + prune logging + banner split shipped; airlock/lap root cause still open"
metadata:
  type: project
---

Two field incidents 2026-09-04, "storage degraded" on Emma's Android and Nick's Leviathan.

**Emma's Android:** benign one-shot — install kill mid-flush → 1 dangling pointer → repair pruned it → self-healed. **Leviathan:** 7.5h of failing writes (`seal verification failed` on chains/settings/messages, ACKs withheld) ending only at a repair that pruned **43 dangling pointers**. Hardware FULLY exonerated: btrfs datasum on + corruption_errs 0, SMART clean (Samsung 840 PRO on /dev/sdb3 — first smartctl polled the wrong disk), dmesg clean, RAM memtested. CAVEAT that held: clean btrfs rules out the disk, not RAM.

**CONVICTED — LIVE-LAP, not corruption, not interrupted grow.** Raw scan: zero corrupt blocks in either vault; the bytes were always fine, the REFERENCES were wrong. The mid-grow hypothesis died (4352 = 4096 + 4·reap_window = the AIRLOCK grow in `rescue()`, vault.rs — a deliberate size, not a tear; grow's crash window is documented harmless). The airlock firing at all means Leviathan was WEDGED (reap parked on live cluster). The bounded inspector then showed the smoking gun: the ~50 mismatched pointers cluster in TWO CONTIGUOUS BANDS (lba 403-439 and 3065-3098) — two multi-block append runs written straight over territory the committed index still referenced. Same family as the 2026-08-24 "214 lost blocks" (hamt.rs repoint-miss), whose loud-on-miss guard did NOT stop this — some path still laps.

**FIXES SHIPPED 2026-09-04 (manifestus + kete + photon):**
1. **THE LAP GUARD** — `Tract::append` now takes `&dyn Liveness` (new trait method `is_referenced(lba)`) and REFUSES to write onto any lba in the live map: emits `StorageEvent::LiveLapAverted`, returns TractFull, the ladder's grow-barrier realigns onto provably clean space, the value survives. Threaded thru hamt put/flush/reap; `NoLive` oracle for tests/genesis. Any lap path, known or unknown, is now loud-and-harmless instead of silent loss.
2. **Airlock grow doubles when live-heavy** (live·2 > len): the old `len + 4·reap_window` was a token on a ~full tract — consumed at once, re-wedge, loop = the 7.5 hours. Dead-heavy wedges keep the modest grow.
3. **kete logs every pruned pointer** (lba+hash+key+route+reason) — the 43 identities were lost forever to a count-only log line. Also wired `LogSink` into the mirror event sink (closes manifestus's "emit-site wiring" TODO); `LiveLapAverted` logs at error level.
4. **Banner split in photon:** amber "storage degraded" (`vault_sick`, distrust) vs RED "storage lost data" (`vault_data_lost`: pruned values or vault won't open) — Msg::StorageDataLost in en/es/mi. Emma's and Leviathan's incidents no longer wear the same words.
5. **Inspector bounded** — the vaultinfo hang on the reproducer was `inspect::walk_tree` (NOT `head_search`): no cycle guard + descending thru mismatched seals into reused blocks = exponential breadth. Now: revisited lba ⇒ "cycle, pruned" note; !seal_ok nodes don't descend. Reproducer went from infinite to 0.17s with a full forensic report.

**Reproducer kept:** `~/Downloads/corrupted vault.vsf` (18MB pre-repair snapshot copy). `vaultinfo` now names all bands.

**STILL OPEN:** (a) the actual lapping path — WHAT wrote those two bands over live territory despite the fence + repoint guards? The lap guard converts it to a loud refusal, so the next occurrence will name itself in the log (`LIVE-LAP AVERTED at lba N`); watch field logs for it. (b) pre-existing test failure `migrating_survives_kill_nine_with_exact_committed_prefix` (fails on CLEAN tree, "at least one migration happened under fire", root_gen stays 0 — not from these changes). Related: [[project_manifestus_tombstone_bug]], [[project_storage_layering]], [[project_vault_op_latency]].
