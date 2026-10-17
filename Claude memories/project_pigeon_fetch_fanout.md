---
name: project_pigeon_fetch_fanout
description: "2026-09-17 pigeon re-uploads CONVICTED+FIXED: fetch asks ONE ranked device per round, chunks carry a relay copy only when the direct leg is unproven, 10 s serve dedupe, held blobs ignored"
metadata:
  type: project
---

Field 2026-09-17: two ~142 MB pigeons were re-served in full (68 MB twice in 3 s); small pigeons completed ~9× each.
Causes: `attach_fetch` asked every device at once and every holder served the whole blob; every served chunk also rode a relay copy unconditionally (the history drain sends direct + relay for each `relay_to`); duplicate asks were each served; held blobs were re-stored.
Fix: `attach_fetch` ranks candidates (friend online + validated path → friend online → our online siblings → rest) and asks ONE, rotating on the re-ask; the serve sets `relay_to` only when the request came via relay or no validated path exists (the tiny manifest always copies); `attach_served_recent` drops a repeat (device, hash) ask within 10 s; manifest/chunk arms bail on `blob_present`.
History pages still always carry the relay copy — revisit if relay cost shows there.

**Why:** Nick: "I keep seeing re-uploads/replications of pigeons" — every duplicate is bandwidth and Cloudflare cost.
**How to apply:** one asker and one server per round; relay legs only when the direct leg is unproven. Related: [[project_pigeon_fetch_reach]], [[project_attachments]].
