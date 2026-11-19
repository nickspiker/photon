---
name: feedback_bootstrap_last_resort
description: "HARD: everything comes from peers (FGTW the network); fgtw.org only when no LAN broadcast is heard AND the WAN reaches nobody, or a brand-new device attests"
metadata:
  node_type: memory
  type: feedback
  originSessionId: 17b90e8e-8b1a-448e-8ac7-22b057e9a0d7
  modified: 2026-11-19T01:21:19.125Z
---

Owner, 2026-11-18, "for likely the fifth time now": "ideally we get EVERYTHING without the bootstrap. If I don't see anyone on the LAN with a broadcast and my WAN is not hitting anything, then we should probably use the bootstrap, otherwise? FGTW baby!!!"
Earlier the same day: "the bootstrap is literally supposed to be a fallback so since I can obviously pull the phonebook from my other devices I shouldn't even need the bootstrap." And 2026-11-10 (the phonebook-web plan): fgtw.org is contacted only to attest a brand-new device, when every peer is offline, or when every peer's address changed and nobody answers.
**Why:** FGTW is the Fractal Gradient Trust Web; the peers are the network, fgtw.org only seeds it. Every design that reaches for fgtw.org first makes Photon depend on one server.
**How to apply:**
- Any new data path (addresses, phonebook, chains, avatars, settings, rosters, history, attachments) asks peers first: own siblings, then friends, then any peer that holds it; fgtw.org is the last rung, taken only on the edge "no LAN broadcast heard and no WAN peer answers", or for a first attest.
- Never propose fgtw.org (or a TCP/HTTP path to it) as the fix for "a device can't find X"; the fix is a peer handing X over. A path to fgtw.org is still worth having as the fallback, but say it is the fallback.
- When reviewing or briefing agents, check this explicitly.
Related: [[project_peers_are_fgtw]], the plan at ~/.claude/plans/purrfect-cooking-moth.md (phonebook web), [[edges-not-timers]].
**Releases too (owner, 2026-11-19):** "I don't want to host this, I want nodes to share the new version among themselves, that's long vision but short vision is use the communications network we built to communicate, not cloudflare, that should literally be a list of IP's and devices, that's why I call it a phonebook." Short term: a device that learns of a signed release tells its fleet and friends over the Photon network; each verifies the release signature itself. Long term: peers hand each other the signed binaries; R2/fgtw.org only as the fallback.
