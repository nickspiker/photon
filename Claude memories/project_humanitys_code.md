---
name: project-humanitys-code
description: The openness doctrine - everything is public except handles and keys; "this is not my code, this is humanity's code"
metadata:
  type: project
---

Nick, 2026-08-16, settling the memory-migration privacy line: "The map lives in keys, otherwise everything aside from handles should be very public. This is not my code, this is humanity's code."

**The rule:** the secrecy surface is EXACTLY two things — handles (the strings derive identity seeds) and key material (keys/, mirrored privately). Everything else defaults to public: incident reports, design docs, threat models, memory files, field failures, dead ends, the embarrassing bugs and the fixes alike.

**Why:** the project is a commons. Hedging internals as if they were trade secrets would betray what it is; the honest record of how it was built — including what went wrong — is part of what is being given.

**How to apply:** when hesitating over whether something belongs in the public repo, the question is only "is it a handle or a key?" If neither, publish. Pseudonymize people (the stable first-name convention, map in keys/), then write plainly.
