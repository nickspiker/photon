---
name: project_doorbell
description: "Reachability doorbell v1 BUILT 2026-07-19 (photon f852cbb + worker f3d621f, FCM_SERVICE_ACCOUNT secret set): last_heard clock, dozed ring triggers, bell registry, direct FCM v1 sender, Kotlin wake; NOT built: TCP-keepalive tier-1, per-device bell keying, opt-in toggle, UnifiedPush client; E2E untested"
metadata: 
  node_type: memory
  type: project
  originSessionId: da87e023-e95d-4d82-8020-abdabc41164d
---

Doorbell v1 shipped 2026-07-19 per docs/reachability-doorbell.md (status header there is canonical). Wire contract: bells are `kind:address` strings (`fcm:<project>:<token>`, `up:<url>`), published preference-ordered + device-signed under handle_proof via `bell_put` (repeated `bell` fields, worker reads get_fields); `ring` = self-consistent-signed target hp → worker walks bells → empty HIGH-priority FCM data push `{type:wake}` (RS256 JWT via WebCrypto, token cached per isolate) or empty POST to `up:`; 30s per-target R2 ring guard + client debounce (90s dozed threshold on `Contact::last_heard`, 5min `last_ring` re-ring).

Triggers: due chat retransmit to a silent peer (retransmit_due_messages) + Pending-ceremony-parked-no-path branch (ping_contacts). last_heard stamps: Online arm (positive only — TIMEOUT rides the same arm with is_online=false), PathValidated arm. Android: token→JNI (`nativeSetFcmToken` from both services, project id off baked google-services.json), ping-cycle publishes/republishes on rotation; Kotlin wake: warm→`PhotonConnectionService.live?.requestServiceTick()`, cold→generic notification (no session until the [[project_android_session_capsule]] boot capsule exists).

OPEN (v1 limits, deliberate): bells hp-keyed so FLEET SIBLINGS OVERWRITE each other's bell (per-device keying = bells/<hp>/<dev> later); Android publishes unconditionally — the honest-copy opt-in settings row (off-by-default per the doc) is NOT built; TCP/WS-keepalive background tier-1 not built (UDP ping treadmill still runs backgrounded); UnifiedPush client registration not built (worker rings `up:` already); E2E never tested (needs a dozed phone). Worker secret: `FCM_SERVICE_ACCOUNT` on the `fgtw` worker = the fgtw-90220 service-account JSON; key id 561862daf2 passed thru a chat context once — rotation = Firebase console new key + `wrangler secret put`, no code change.
