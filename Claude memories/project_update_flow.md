---
name: project_update_flow
description: "Self-update flow BUILT (manual + automatic + stamp window @ 228f68c 2026-07-16) + release-notice push BUILT 2026-07-19 (deploy.sh curl -> worker hub broadcast + FCM updates topic -> instant manifest poll); remaining: Android platform notification, rollback, idle gating"
metadata: 
  node_type: memory
  type: project
  originSessionId: 5bffb0bc-fa36-44f3-979a-7be0e42db118
---

Live-update flow written up 2026-07-10 in `docs/updates.md` (status: DESIGN, nothing built).
The user kept feeling "we discussed this" — it had only ever been verbal, so it kept evaporating; now captured.

Agreed shape:
- **Desktop = hot self-replace.** Poll a VSF-signed version manifest (release key, the one `photon-keygen` makes + binaries self-verify against) → download new signed binary → verify on disk BEFORE exec → atomic swap (Unix `rename()` over the path; Windows rename running exe to `photon.old`, write new, delete `.old` next launch) → `execv` re-exec. Transparent because session (tohu registers) + vault persist; re-exec lands on the same screen.
- **Android = notification → system installer.** OS forbids silent APK self-replace; ceiling is "update available" → tap → package installer. User already accepts this.
- **The invariant:** verify-before-exec is the WHOLE security boundary (the update channel is an RCE vector). Plus the STAMP WINDOW (decided 2026-07-12): version = release stamp `t`, accept iff `floor < t ≤ now`. Lower bound = no downgrade replay; upper bound = no forward-dated stamps, so the floor can never outrun real time and a far-future-stamped build can't version-ceiling-brick the client. Forward-dated means "not yet" (re-evaluated next poll), so no skew tolerance parameter exists — a lagging clock just delays. Floor = the running build's embedded stamp, never mutable stored state. Cost: clock enters the boundary — and the answer (decided 2026-07-12) is `now` = nunc consensus at the accept/defer decision point (see [[project_nunc_clock_check]]), NOT the system clock. Rationale: a locally-dishonest clock is out of the threat model (self-harm only, forward stamps only exist under key compromise); the real adversary is network clock-bending — clock-back = silent freeze the upper bound itself introduces, clock-forward+stolen-key = floor poisoning surviving key rotation. Fail-safe composes: no consensus → defer = "not yet". Compare against the conservative confidence edge. USER MANDATE: nunc-`now` on EVERY platform, no system-clock fallback anywhere (most users are Android; a fallback there re-opens the attacks on the majority platform) — nunc un-gated on Android 2026-07-12 to satisfy this; Redox defers all updates until nunc gets a pure-Rust TLS provider. This makes nunc load-bearing for updates, beyond warn-only banner duty.

Two things I added beyond the user's sketch: downgrade protection (the monotonic check) and in-flight-textbox hand-off across the exec (so it's fully transparent, not "aside from unsent typing").
Reuses existing: Ed25519 binary signing + startup self-verify, R2 distribution, FGTW/VSF signed docs, durable session+vault persistence.

**BUILT 2026-07-16** (manual flow earlier that week; automatic + stamp window @ 228f68c):
- Manual: Updates page auto-checks both channels on open; two buttons install on ANY version difference (explicit channel hop, downgrade by user intent — deliberately not window-gated); desktop swap+re-exec, Android → system installer; CDN cache-bust by content hash.
- Stamp window AS BUILT, reconciled to the counter version scheme (the "version = stamp" design line predates major.minor.patch): `t` = the signed manifest HEADER's creation stamp (e6, inside the signature), floor = `PHOTON_BUILD_STAMP` (build.rs, eagle osc at build; vsf is a build-dep so the clocks can't drift). Forward-only ordering rides the version tuple.
- `now` STAGED (not always-nunc — softening of the earlier mandate that keeps nunc load-bearing): system eagle time happy-path; the retained nunc verdict (`clock_consensus`, conservative edge offset−confidence) arbitrates every forward failure; no verdict → spawn consensus + defer.
- Automatic: `drive_auto_update` in tick — jittered ~6–8h release-channel poll (first ~1min after launch), gated by fleet-synced `updates.auto` (Updates-page checkbox, default ON). Desktop RELEASE builds (patch==0) self-apply+re-exec; dev builds NEVER auto (user mandate: dev is manual) and Android toast once per version. Dev channel never polled automatically.
- Publish integrity: manifest stamps pinned at bump + flock-serialized publishes (58bf7ed) after the 2026-07-16 race (a v11 APK published claiming v12 + the wrong commit — overlapping dev-*.sh runs).
**Release-notice push BUILT 2026-07-19** (photon 1e3fb1f + worker 7f681a9): deploy.sh post-publish curls `/admin/release-notice?auth=<activity token>`; worker broadcasts a `release` pair_evt over the WS hub (every RUNNING client sets `next_update_check_osc = 1` -> poll now) + one FCM v1 send to the `updates` topic (dozed Android; Kotlin subscribes at token fetch, `type=update` sets a one-shot drained past drive_auto_update's gates). ADVISORY ONLY by design — the notice can at worst cause a signed-manifest poll; the stamp window still gates installs. Endpoint smoke-tested live (fcm topic ok = the RS256/OAuth/FCM-v1 chain works). Shares the doorbell sender ([[project_doorbell]]).
Remaining (ticketed): Android platform notification (toast only today), rollback-on-crash (.prev), idle gating + textbox hand-off across exec.
