---
name: push-after-landing
description: "Push photon (code + 'Claude memories/') after landing work; the private photon-claude-memory repo is RETIRED 2026-08-23 — memories ride THIS repo on every machine"
metadata:
  type: feedback
---

After landing work: commit and push photon. Memories are part of photon, so a memory write is committed and pushed WITH the session's photon commits, never to a side repo. No machine is ever the only copy (the MacBook is wiped often; the desktop holds Harbor/Chiton/MEGA mirrors — [[reference_backups]]).

**Why:** the retired private memory repo (photon-claude-memory) contradicted [[project_humanitys_code]] (nothing is private but keys + handles) and its history accumulated real handles ([[no-private-handles]]). Folded into 'Claude memories/' and deleted 2026-08-23; if a stray clone resurfaces, it is superseded — never resurrect it.

**How to apply:**
- Where the harness memory dir is a symlink into the photon checkout, `git pull` photon before acting on memory each session, and never leave memory edits uncommitted at session end.
- Release tags v36–v54 were deleted 2026-08-23 (they pinned commits whose messages carried handles). In any clone still holding them: `git tag -d`, and never run a bare `git push --tags` from it.
Related: [[macbook-trails-remote]], [[persist-findings-early]].
