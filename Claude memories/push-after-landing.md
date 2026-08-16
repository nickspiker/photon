---
name: push-after-landing
description: "push photon after committing, and commit+push the memory repo after memory writes — this MacBook is the only copy of neither, ever again"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 81588914-5914-4600-bb98-72cc4fae2260
  modified: 2026-08-14T00:36:01.517Z
---

2026-08-13: photon sat 24 commits ahead of origin (a full week of wedge fixes existed only on the travelling MacBook) and the memory directory had NO git at all. Nick: "so I don't drop this macbook in the ocean and they are lost?" — both are now remote-backed.

**Why:** the MacBook travels (hotels, airports, oceans); local-only state is one accident from gone.

**How to apply:** after landing photon commits in a session, `git push` before the session ends (memory [[nick-publishes]] allows commit/push — only publish scripts are Nick's). After writing/updating memory files, `git -C ~/.claude/projects/-Users-nick-Code-photon/memory add -A && commit && push` — the remote is the PRIVATE repo github.com/nickspiker/photon-claude-memory (must stay private: memories name handles and family roles the public photon repo must never carry, see [[no-private-handles]]).

**2026-08-13 correction (the second fuckup):** this repo is the SHARED corpus for ALL of Nick's machines, not the MacBook's. The desktop (`/mnt/Octopus/Code/photon`, memory dir `-mnt-Octopus-Code-photon`) holds its own richer memory set (`project_*` naming: bridge, chain_advance_desync, fleet_unification_v1, clutch_token_asymmetry, …) that predates this repo — merge instructions live in README.md (union merge, MEMORY.md is the only conflict). `git pull` the memory repo BEFORE acting on memory each session — another machine may have pushed. Until the desktop merge lands, treat the desktop corpus as unseen context: its memories on chain replication / CLUTCH tokens overlap this week's per-device-lanes surgery and must be read for contradictions once visible.
