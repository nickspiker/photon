---
name: project_attest_to_contacts_latency
description: SHELVED 2026-11-10 but MUST be fixed: 18 s from attest to the contact list on the owner's device
metadata:
  type: project
---
Nick 2026-11-10: "18 seconds from attest to contacts. That's not a good number. (shelf that until the rest is complete but that NEEDS improved!)"
Measured on ANDROID; Nick submitted a log for it on 2026-11-10 (pull it with photonlog --pull, see [[reference_log_pull]], when this is picked up).

**Why:** first paint of the contact list after attest is the app's front door; 18 s is unacceptable.

**How to apply:** after the fgtw flow machines ([[feedback_fgtw_out_of_photon]], fgtw FLOWS.md) and the Pending fix ([[project_profile_pending]]) land, profile the attest -> Ready path from the log (the post-attest list in launch.rs on_query_result: fleet event sub, lock reconcile, declare, scheme refresh, fleet key sync, roster pull/push, sibling load, chain load, contact fleet refresh; vault open; contact/conversation load) and move every network wait off the path to the first paint. Related: [[project_ready_first_paint]], [[feedback_render_never_touches_vault]].
