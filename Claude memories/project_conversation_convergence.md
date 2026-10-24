---
name: project-conversation-convergence
description: One conversation model at any size — plan in docs/conversations.md; Phase 0 (fold-checked roster records) BUILT 2026-10-24, Phase 1 (one UI surface) next
metadata:
  type: project
---

The friendship/molecule split converges per docs/conversations.md (2026-10-24), driven by [[feedback-any-size-conversation]].

Model: a BOND per pair of people (today's Contact + pairwise 12-primitive CLUTCH: consent, identity binding, PQ; carries only control traffic; siblings are bonds) and ONE Conversation type for N = 1, 2, more (stable id, signed roster, keys minted and wrapped to each standing device's bundle, same JOIN for every member past the founders). A literal N-party CLUTCH is O(N²) with everyone online, so "CLUTCH for any size" = bond CLUTCH + per-conversation mint/join/wrap, shown as one ladder.

Phases: 0 groundwork (BUILT f3fb014d: records verify against their SUBJECT's fold via `partition_by_fold`; unheld folds park in `molecule_parked` and re-merge on fold landing; golden test pins id/token/era derivations) → 1 one surface (ConvView, one panel Bonds·Stats·Manage + Person drill-in, one status ladder, one Ready row, invitations as Ready rows) → 2 one model (types/storage/send/receive/ACK, wire-neutral migration) → 3 one key agreement (flag day: bond split, mint/wrap eras for every size, one-shot bundles, invites ride bonds).

D8 (Nick 2026-10-25): PEOPLE and CONVERSATIONS are separate — adding a friend creates the bond only, never a conversation; Ready = conversations (All), People = who you added, Invitations; tapping a person = Message (opens or founds the two-member conversation).

Found while mapping: the "withhold a short wrap set" tripwire the mint relies on was never built (comments now say so); a never-friended member's party↔handle_proof binding is trust-on-first-sight thru the sponsor.

**Why:** two code paths for one concept is where the divergence bugs live (Emma's join, the missing crypto status on molecules).
**How to apply:** build each phase so the app works at every step; owner decisions D1–D7 are listed in the doc §6 — ask before Phase 3 crypto.

Field round 3 (2026-10-24, v0.125.9-10): binding worked but showed on one device, members stranded, files never sent, no ding. Convicted + fixed in b0aab1a1: queued mint wraps drained AFTER cutover (sent under the new era) and a held control row re-sent as a chat row; a Bind placeholder (all-zero root) refused a sibling's real keys as an "elder era" (`awaiting_root` now yields); attach send keyed on a contact index, so `active_contact()` = None in a molecule dropped it. OPEN: a stranded member that never writes is only healed when it sends (retired-era frame trigger); group eras over bonds not built.
