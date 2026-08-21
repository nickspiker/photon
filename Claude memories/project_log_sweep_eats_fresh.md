---
name: project-log-sweep-eats-fresh
description: "fgtw worker :17 hourly retention sweep DELETED an 18-min-old 2.8MB log submission (24h rule violated) while sparing same-window objects; instrumented worker deployed 2026-08-21, tail armed for the 14:17Z cron; planned fix = compare key-embedded eagle osc, not workers-rs uploaded()"
metadata: 
  node_type: memory
  type: project
  originSessionId: d83fbeaf-685c-4da4-8647-7b49de82fd2c
---

2026-08-21: Nick's 12:59:21Z log submission (photon-logs/11c80922…/2558864839399754752-dev90e571bf.vsf, 2.8MB) existed at 13:06Z and was GONE at 13:18Z — the only deleter in that window is the worker's :17 hourly retention sweep (scheduled handler in fgtw-bootstrap lib.rs), whose rule is "delete >24h old". Four fresh 13:05Z smoke objects under a different tag SURVIVED the same sweep. Nick reports the same vanish-after-submit the previous session — so submissions may only survive until the next :17.
Suspects: workers-rs 0.4.2 `Object::uploaded()` misbehaving on list results in the prod runtime (Nick's object was lexicographically FIRST — consistent with delete-until-death/limit), or an isolate abort mid-loop.
Instrumented worker (per-object key/uploaded_ms/verdict console_log) DEPLOYED 2026-08-21 ~13:28Z; `wrangler tail fgtw` running in background to catch the 14:17Z cron with 6 bait objects in the bucket (4 smoke + 2 sweeptest under photon-logs/0000testtag/).
Planned fix regardless of verdict: the object key already embeds the upload eagle-osc (`<osc>-dev<tag>.vsf`) — compare THAT against now-osc minus 24h, pure numbers from our own format, no JS Date interop. Then remove the diagnostic logging, delete the bait objects, remove wrangler.sweeptest.toml + the preview_bucket_name line (scratch, uncommitted), redeploy.
Also open: `wrangler dev --remote` unusable on this account ("Could not create remote preview session"), and the deleted 2.8MB submission held Nick's 3-4 ~1s desktop hang captures — evidence LOST (local copy died on close pre-flush-fix); next occurrence is catchable since quit is a flush edge (photon b58312b).
The retrieval-tag identity check came out CLEAN: the desktop session tag equals the derivation from the real handle exactly (the transient "mismatch" was the map-column trap in [[reference-log-pull]]) — the sweep is the sole culprit.
Related: [[reference-log-pull]] (--session flag + map-column trap), [[project-render-storm-lag]].
