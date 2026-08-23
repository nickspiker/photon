---
name: push-after-landing
description: "Push photon (code + 'Claude memories/') after landing work; the private photon-claude-memory repo is RETIRED 2026-08-23 — memories ride THIS repo on every machine"
metadata: 
  node_type: memory
  type: feedback
  modified: 2026-08-23
---

After landing work: commit and push photon — and memories are part of photon now, so a memory write is committed and pushed WITH the session's photon commits, not to any side repo. No machine is ever the only copy (the MacBook gets wiped frequently; the desktop holds Harbor/Chiton/MEGA mirrors — [[reference_backups]]).

**History:** the MacBook briefly kept a separate PRIVATE memory repo (photon-claude-memory). That was the wrong model twice over — it contradicted [[project_humanitys_code]] (nothing is private but keys + handles), and its history accumulated actual handles ([[no-private-handles]]). Folded into 'Claude memories/' and DELETED 2026-08-23. If a stray clone of it ever resurfaces, its content is superseded — do not resurrect it.

**Mechanics on a machine whose harness memory dir is a symlink into the photon checkout (the MacBook since 2026-08-23):** memory writes land directly in the working tree — `git pull` photon before acting on memory each session (another machine may have pushed), and never leave memory edits uncommitted at session end. Related: [[macbook-trails-remote]], [[persist-findings-early]].

**Tag quarantine (2026-08-23):** release tags v36–v54 were DELETED — they pinned the pre-rewrite line whose commit MESSAGES carried handles ([[no-private-handles]]). If any clone still holds them: delete locally (`git tag -d`), and never run a bare `git push --tags` from such a clone — it would resurrect the dirty commits on the fresh remote.
