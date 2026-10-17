---
name: project-vault-op-latency
description: "CLOSED 2026-08-21: every vault put on the Linux desktop cost ~0.9-1.1 s flat (per-op commit+fsync); click-a-contact hang = reads queued behind it; group commit + off-UI writes fixed it"
metadata:
  type: project
---

2026-08-21, via kete's librarian slow-op probe: vault puts on Leviathan (btrfs, dual mirror rings) cost ~850-1130 ms each REGARDLESS of size (53 B = 857 ms). Every caller blocks on the librarian, so conversation-open reads waited behind in-flight writes = the click-a-contact hang; the Mac never showed it.

Fixed same evening:
1. GROUP COMMIT: manifestus `put_batch` (N puts, one spine generation) + kete librarian batch drain (MAX_BATCH 64).
2. Five UI-thread inline vault writes moved to the seal room (photon b7dd871).
3. The fstate "ping-pong" was correct CRDT churn from an era wedge, not a bug.

**How to apply:** a UI hitch near storage = suspect a synchronous vault op on the UI thread first ([[feedback_render_never_touches_vault]]).
Related: [[project_render_storm_lag]], [[project_self_message_vanish]] (the later batch-commit arc).
