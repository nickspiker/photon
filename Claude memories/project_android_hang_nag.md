---
name: project_android_hang_nag
description: "CLOSED 2026-09-12 — Android launch ANR = vault open on the UI thread; open now runs on a vault-open worker and reads the index only (manifestus c37b511)"
metadata:
  type: project
---

Pixel 8 ANR at nearly every launch (2026-09-10/12): the main thread sat ~5 s in the vault open (manifestus walking and hashing every block, growing with kept waves).
Fixed in three steps: manifestus bd1e12a (converged mirrors skip the live-walk); photon `open_session_vault` on a `vault-open` worker, `poll_resume_vault` → `finish_resume_load` in tick; manifestus c37b511 (`walk_tree_collect` reads the index only, furrows marked live by position via `LiveSet::referenced`) — 23 reads to open 1,025 live blocks.
Nick: "treed open was the initial design; zero reason we verify the whole vault." Next per Nick: conversations most-recent-first, old content trimmed as the ring rotates.

**Why:** the ANR threshold is 5 s of unanswered input, and vault-size-proportional work crossed it as recordings piled up.
**How to apply:** read `PERF: resume load` first in any phone log (a vault number near 5000 ms = this class); vault-size-proportional work never runs on the UI thread ([[feedback_render_never_touches_vault]]). Symbolizing a native stack: [[project_crash_handler_sigchain]].
