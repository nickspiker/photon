---
name: project_emma_brittany_group_fork
description: OPEN 2026-11-01 — Emma's phone drops every frame from Brittany's c9b5f417 (377 since 01:50 UTC): same index 1, different root (Brittany f6e94668 vs Emma 256da35c); likely the new atom "blue" 5b0bef4e
metadata:
  type: project
---
Emma's phone log 2026-11-01: "CHAT: frame tagged era f6e94668 from c9b5f417 — ours 256da35c, retired none, pending none — dropped", 377 times from 01:50:15 UTC, and "c9b5f417 advertises era#f6e94668 (index 1) for a friendship we hold at era#256da35c (index 1)".
Same index, different root = two era-1 mints of one group. Emma founded atom "blue" (5b0bef4e) at 01:48:25 and minted era 1 for it at 01:48:34 ("1 to a joiner; pending until the first wrap ACKs"), Brittany joining — the likely fid; not confirmed (the log never names the fid on the drop line).
Not the bond fix (docs/lanes.md "Which era is live" covers bonds and two-member conversations); a molecule fork needs the group's minter rules (docs/molecules.md §10).

**Why:** Brittany's rows in that group never reach Emma, silently.
**How to apply:** confirm the fid (add it to the drop line), then look at how two era-1 mints arose for one group; related [[project_emma_bond_era_split]], [[project-molecules-build]].
