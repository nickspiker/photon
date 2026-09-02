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

**STILL OPEN (in leverage order):**
1. **Vault engine**: kete is commit-per-write (group commit added 1944ebe + REVERTED d48f294 same day 2026-08-21 — batching starved manifestus's rollback fence; put_batch is dead code). One put = 8-12 fdatasyncs across two mirrored rings; BTRFS CoW ~14.8K extents/ring inflates each (2-5s SLOW puts in field). Fix direction: rarangi transact → one manifestus put_batch commit (WAL is the recovery story, so batching apply is safe by design); reconsider fence-aware bounded batch in kete.
2. **PT frozen addresses**: small-packet retransmits + DATA fire forever at addresses frozen at enqueue (pt/mod.rs:770-794); TRAVERSE path-validated addresses never re-target them; stop-and-wait head-of-line per address. Inbound transfers keyed by source addr (mod.rs:350) → SPEC-via-IPv6 + DATA-via-v4 = permanent unknown-drop. Field: desktop sprayed stale wrong-subnet 192.168.0.40 + phone's dead cellular v6 + a blocked v4 while validated v6 sat idle. Same class as the packet-hash ACK fix (mod.rs:316 comment). Completion dispatcher also drops unknown payload kinds silently (33910B/183KB frames, every receiver, senders retry forever) — same class as the av_resp fix (status.rs:2041).
3. **Ghost device 1be949c1** (phone's pre-re-attest id) still in the published fleet membership chain → permanent dead fan-out leg (50× relay drops). Related [[project_call_no_ring_incident]].
4. Message delete still runs full-table save_messages sync on UI thread (driver.rs:2894).

Phone cacbc223 runs a STALE build (APK 15:26 < fixes) — needs scripts/android/dev-adb.sh rebuild before any self→phone delivery works.

**2026-09-02 23:48 wipe**: Nick []x-nuked desktop, re-attested 23:49:08, FLEET-HOLDS-HISTORY backfilled everything (13 pages, "47 new of 47"…) — zero loss fleet-wide even after full local wipe. The durability model held; the transport made it slow ("sync finally happened" ≈ minutes).

Call glare (same day): Emma+Nick dialed simultaneously → mutual auto-BUSY, no ring, no notification; offer fires to only 1 path. Unfixed.
