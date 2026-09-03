---
name: project_self_message_vanish
description: self-message 40s-to-minutes latency + restart vanish ROOT-CAUSED (21-verdict adversarial sweep 2026-09-02) — delta gate + quit drain SHIPPED 68b1912; vault engine + PT re-target + ghost device still open
metadata: 
  node_type: memory
  type: project
  originSessionId: 8c6221f3-48a5-4f77-b00e-1e3c7fa4c3b5
---

Field day 2026-09-02 ("ten-thousand-year letter"). Workflow sweep (4 investigators + adversarial verifiers, 21 verdicts) + live stuck-message forensics. My first theory (is_sibling ephemeral gate eats fleet chat) was WRONG for Notes-to-self — that contact is NOT is_sibling; the gate only covers per-device `$ ` bridge contacts, correctly.

**CONVICTED + FIXED (photon 68b1912):**
- **~300-commit amplification**: save_messages re-put EVERY row every save (72-row table × 4-5 durable commits/row via rarangi WAL/row/catalog/superroot/retire); kete fresh-random-nonce AEAD defeats identical-overwrite skip. FIXED: delta gate in save_messages + save_messages_page (compare would-be Record vs decoded durable row — Record: PartialEq, width-tolerant decode round-trips equal — skip identical). save_messages returns (written, skipped); regression test pins (5,0)/(0,5)/(1,5)/delivered-flip.
- **Quit ate queued writes**: process exited with snapshots in the message/chains writer queues (the vanish: 73rd row merged 22:59:15, persist enqueued, quit killed it — never-written). FIXED: durable_pending Arc<(Mutex<usize>,Condvar)> counts enqueues on message+chains writers, dec AFTER write lands; drain_durable_writers() blocks deliberate-quit/non-resident-close/update-re-exec edges until zero. Edges not timers.
- Zero-remote design itself is CORRECT: self send = vault write IS delivery, bright on persist verdict (messaging.rs:132-146) — identical machinery as friends, per the hard rule.

**ENGINE + TRANSPORT FIXES SHIPPED 2026-09-02 (four repos):**
- **manifestus 660c66d**: apply_batch — mixed puts+deletes, order-preserving, ONE spine commit; fence-safe (put ladder already commits mid-ladder on Fenced — the revert's homework was done, never wired).
- **kete 7fabf12**: librarian gathers EVERY queued mutation per drain into one apply_batch commit (safe 1944ebe: commits within the cycle, never held open) + apply_addr_batch API + entry_addr; dead put_growing removed.
- **rarangi 2ac9e8a**: transact apply = one ordered batch [post-images, superroot, WAL retire]; WAL stays atomicity anchor. 4-5 commits/transaction → 2. With delta gate: one send ≈ 2 commits (was ~300).
- **photon 115a346**: PT frozen-address fix — retarget_peer on verified pong (re-aims queued packets + un-locked transfers by recipient pubkey), handle_data unique-stream fallback with address adoption (SPEC-via-v6 + DATA-via-v4 wedge), handle_ack/handle_spec_ack third-address lock. 3 regression tests.

**STILL OPEN:**
1. **Ghost device 1be949c1** in the published membership chain → dead fan-out leg. Design note SHIPPED docs/ghost-device-supersession.md (recommend: routing ostracism now, hardware-continuity supersession in fleet-key redesign). Awaiting Nick.
2. Message delete still runs full-table save_messages sync on UI thread (driver.rs:2894).
3. Completion dispatcher unknown payload kinds: scout says 33910B/183KB were AvatarResponse (since wired); re-check field logs post-fix for residual "claimed by NO parser".

Phone cacbc223 runs a STALE build (APK 15:26 < fixes) — needs scripts/android/dev-adb.sh rebuild before any self→phone delivery works.

**2026-09-02 23:48 wipe**: Nick []x-nuked desktop, re-attested 23:49:08, FLEET-HOLDS-HISTORY backfilled everything (13 pages, "47 new of 47"…) — zero loss fleet-wide even after full local wipe. The durability model held; the transport made it slow ("sync finally happened" ≈ minutes).

Call glare (same day): Emma+Nick dialed simultaneously → mutual auto-BUSY, no ring, no notification; offer fires to only 1 path. Unfixed.
