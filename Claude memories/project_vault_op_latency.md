---
name: project-vault-op-latency
description: "CONVICTED 2026-08-21: every vault put on the Linux desktop costs ~850-1130ms FLAT regardless of size (53B = 857ms) — per-op commit/fsync on both mirror rings; the click-a-contact hang = open-path reads behind the permanently-occupied write queue (fstate ping-pong storm); fix order = ping-pong, then librarian GROUP COMMIT, then UI-drain write audit"
metadata:
  type: project
---

The 2026-08-21 hang arc's terminal finding, via kete's librarian slow-op probe (kete 63edff4) on the first quit-flush local capture: vault puts are ~850-1130ms each INDEPENDENT of size (53 bytes = 857ms, 22KB = 1130ms) — pure per-operation commit+fsync latency across both mirror rings on BTRFS.
Every vault caller blocks on the librarian's reply, so: click-a-contact hang = the conversation-open path's vault reads waiting behind the in-flight ~900ms write (the fstate reconcile ping-pong keeps one permanently in flight); the 4-6s process-wide log silences = a few queued mutations back-to-back; `PERF: status arm BlindFrameReceived took 890ms (UI thread)` shows a UI drain writing (or waiting) synchronously.
Fix order (TICKETS.md, Messaging section): 1. fstate reconcile ping-pong (set-equality, kills the write volume), 2. librarian/manifestus GROUP COMMIT (drain the queued burst into ONE commit+fsync — the librarian already coalesces structurally, the commit boundary must match), 3. audit UI-thread drains that still write synchronously (BlindFrameReceived first).
Sibling finding, same arc: the Mac (fe46a74b, .152) never hangs — it renders ~106ms full-scene frames under the same event storm; the Linux desktop (90e571bf, .156) is where the vault latency bites. [[project-render-storm-lag]] was the same disease's earlier appearance (vault mutex contention); the mutex moved into the librarian but the blocking remained.
Related: [[project-log-sweep-eats-fresh]] (the arc), [[reference-log-pull]] (--session/--petname pulls, quit-flush).
