# Reachability & the doorbell — waking a dozing peer

> Status: **DESIGN ONLY, not implemented.** Agreed shape from the 2026-07-08 design session.
> The receiving half (FCM `google-services.json` for project `fgtw-90220`) is already in the APK.
> Everything below — the reachability clock, the targeted doorbell, endpoint registration — is net-new.
> The existing `send_fcm_push()` in the worker is the **wrong shape** (target-less topic broadcast, coupled to IP-change) and is replaced, not revived.

## The one physical fact everything hangs off

A dozing phone can be *reached over the network* and still not *hear you*.
The hole is punched, the NAT mapping may still be live, the packet arrives at the radio — and then the kernel drops it, because in Doze **the OS does not schedule the app that owns the socket.**
Nobody runs `recv()`. The packet evaporates.

So the problem was never "get a packet to the phone." It is **"the CPU is asleep and only the OS can wake it, and the OS only wakes a process for a wake-path it blessed."**
A punched hole gets your packet to a sleeping guard who never opens his eyes.

The **doorbell is not a transport.** It is a remote `wake()` — the one channel Android's scheduler is contractually obligated to un-doze a process for. It carries no content, names no sender. It returns the peer to the *scheduled* state where **direct delivery — which you already have — works again.** Then direct delivers.

> **Direct is the transport. The doorbell is a `wake()` syscall for a remote phone.** Content never rides the bell.

## The model: reachability is a property the network provides, not a key it shares

Every other Photon layer is keyless and self-authorizing — identity is derived, the trust web is peer-to-peer, WebPush endpoints are self-authorizing URLs.
A doorbell must not re-introduce a custodian who holds power over the whole userbase.
The resolution: **the device publishes which bells wake it; the sender rings whichever bell it can speak.**
Reachability becomes a *published property*, not *keyring membership*. Bells are fungible; no single one is load-bearing at scale. See [[project_peers_are_fgtw]].

### Why the FCM key stays private — and why that does NOT centralize the design

The FCM service-account key is a **spending credential**: it authenticates as the owner of the `fgtw-90220` cloud tenant and can wake/message *every* device that ever registered with that project, on the owner's bill. It is the opposite of the APK signing key (which is *meant* to be shared with any TOKEN implementer — sharing an identity anchor strengthens it; sharing a spending credential is a breach). So: **FCM key private, worker-secret only, never in the repo.**

"Anyone can host a doorbell" is still true — it just does not mean "share the FCM key." It means the push layer is a **volunteer mesh of fungible bells**:

- **You host FCM** (your private `fgtw-90220` key). The stock-Android floor, ~95% of consumer devices. Your bill, your bell. Convenient, not privileged.
- **A volunteer hosts a WebPush/UnifiedPush relay** (ntfy or self-hosted). Their infra, their bill. The **endpoint URL *is* the capability** — `POST <url>` rings it (RFC 8030 WebPush), no shared secret. This is where "anyone can host a doorbell" literally lives.
- **A volunteer stands up their own FCM tenant** — their own Firebase project + `google-services.json` in their fork's APK. A different app instance to Google, cleanly.

None of these shares a key, because **the capability to ring a given bell is possession of that device's published endpoint, not membership in a keyring.** Volunteers are interchangeable pipes (like Tor relays / DNS resolvers), not custodians of address blocks — there are no address blocks, just devices each publishing "here are N opaque bells that wake me." Your FCM instance is simply the first, biggest volunteer.

## The cascade: try direct, doorbell only when we're pretty sure it's asleep

1. **Direct** over the open hole. Free, instant, works whenever the peer's process is scheduled (foregrounded / recently active / holding a foreground service). Active conversations live entirely here and never ring a bell.
2. **No ACK** in the fast window (~1–2 s) **and** last-heard from the peer exceeds the *dozed threshold* (see below) → classify the peer `dozed` → eligible to ring.
3. **Ring the doorbell** — one empty wake to the peer's published bell.
4. Woken phone **re-punches its hole** (fresh outbound packet = reflexive discovery, per commit 03c759e / [`src/network/traverse/`](../src/network/traverse/)) and re-announces reachability — this folds the "NAT mapping went stale during the doze" case into the wake itself.
5. Sender **retransmits** over the now-fresh mapping → delivered → ACK.

The bell carries nothing; **the hole you already punched does the actual delivery** once the guard is awake. Store-and-forward (relay-and-pull) is a *later, separate* rung, needed only for "phone never wakes at all" (offline / dead battery) — genuine offline delivery is out of scope here.

## Background reachability: TCP-keepalive vs the doorbell

The 30 s UDP ping treadmill (presence + NAT-binding warmth today) is the expensive way to stay reachable: a radio wake every 30 s keeps the modem out of deep sleep — a real battery tax — because UDP NAT mappings die in ~30–60 s of silence.

**TCP/WebSocket bindings survive minutes to tens of minutes**, and Photon already holds a WebSocket to the worker (the FLEET event hub). So background *reachability* moves onto that one TCP/WS connection with ~10–15 min keepalives — the WhatsApp/Signal-websocket shape — and UDP pings demote to **foreground presence only** (and on-demand re-punch when something actually wants to talk). Big battery win over the treadmill, no Google in the loop.

**But TCP-keepalive is a cheaper *tier-1*, not an escape from Doze.** Two things must both stay alive:

1. **The TCP/NAT binding** — survives ~10–30 min. Fine.
2. **The process being scheduled to service the socket** — *this* is the wall. A held TCP socket does **not** exempt the process from Doze. When the OS sleeps the app, the socket stays open at the kernel but **the code never runs to read it and the keepalive timer never fires.**

So it splits exactly by device power-state:

- **Always-scheduled devices** (desktop / laptop — peer-B, the ring/NDK tier): no Doze, socket stays open, 10-min keepalive is trivial. **TCP-keepalive is the complete answer here — no doorbell, no FCM, ever.**
- **Android, backgrounded-but-warm** (recent use, charging, screen-off-but-not-deep): TCP-keepalive on the foreground service works well and is meaningfully cheaper than the 30 s UDP treadmill.
- **Android, deep Doze** (pocket, screen off 30+ min, aggressive OEM): the socket idles, the keepalive misses, the process won't wake. **This — and only this — is what FCM rescues.** TCP does not reach it.

The layering, then, is **not "TCP-keepalive *or* doorbell" — it's both, each doing the one thing it is physically good at:**

1. **TCP/WS keepalive (~10 min)** = primary background reachability while the process is scheduled. Replaces the UDP treadmill; the whole answer on desktop.
2. **UDP** = foreground presence + on-demand re-punch only. Demoted off the background path.
3. **FCM doorbell** = deep-Doze rescue only. Rung of last resort, not the everyday path.

This narrows FCM's job to precisely the case no userspace trick escapes (a fully-dozed Android sleeps the socket reader regardless), and gives tier-1 a cheaper spine everywhere else.

### The "pretty sure it's sleeping" clock — the keystone

There is **no reachability clock today.** What exists is too coarse to reuse:
- [`types/peer.rs`](../src/types/peer.rs) `is_online()` — checks a `ConnectionState` enum only, no time component; goes stale the instant a peer dozes.
- [`network/fgtw/node.rs`](../src/network/fgtw/node.rs) `is_stale()` — a `last_seen` used for **k-bucket eviction** (1 hr `KBUCKET_STALE_OSC`), not messaging reachability.

The doorbell needs a *new* per-conversation clock:

- Track **last ACK/RX from this peer** (any signed traffic: pong, chat ACK, weave probe — all count as "the guard's eyes are open").
- After a direct send, if no ACK within the **fast window** (~1–2 s), the peer is *either* dozed *or* the hole died — we can't yet tell, so we do **not** ring immediately.
- Ring only once **last-heard exceeds the dozed threshold** — "a reasonably long time period" — so a brief packet loss on a live conversation never triggers a wake. Threshold is a tuning knob; start conservative (well past the UDP-mapping lifetime, order of a minute or more) and measure. This is deliberately biased toward *under*-ringing: a missed wake costs latency, an over-ring costs battery and FCM rate-limit budget.

### Debounce — never a bell per packet

The doorbell is **edge-triggered on "I have something for a peer I currently believe is unreachable," then debounced.**
A dozing phone + a chatty sender must **not** produce a bell per dropped packet (battery disaster, FCM throttling).
State is per-*conversation-reachability*, not per-packet: `reachable → dozed → ringing → reachable`. One wake pulls the phone up; once it is up and hole-punched, direct resumes on the free path until it dozes again. While `ringing`, further sends queue silently behind the single outstanding wake.

## What each rung costs, honestly

- **Foreground service (today's tier 1).** A bribe to the scheduler — "keep scheduling me backgrounded." No doorbell needed, hole stays usable, **battery tax** (modem never reaches deep sleep). Works everywhere, always. This is the floor; the doorbell is a *battery optimization* on top of it, not a reachability prerequisite — its absence degrades battery, not function.
- **FCM doorbell.** Lets the process fully doze *and* stay wakeable, by borrowing Google's OS-blessed wake path at ~zero app battery cost. Cost: Google learns **timing/frequency** of your wakes (traffic-analysis surface) — never content, never sender, if the payload stays empty. Custodian = Google (unavoidable on stock Android) + the tenant owner (you). This is why it is the bootstrap, and why it de-googles itself as the rungs below mature.
- **Volunteer WebPush / UnifiedPush.** Same battery economics *iff* the device runs a non-Google distributor holding the socket — so it wins on **de-googled** phones, ties or loses on stock (still rides Google underneath). No central key. The sovereign path.
- **Mesh-relayed wake (later).** A trusted already-awake device nudges the target — but it hits the **same scheduler wall**: it only wakes a phone already holding some OS-blessed wake path. It does **not** replace the doorbell on a stock dozed Android. Fits the de-googled / peers-are-fgtw endgame, not the stock floor.

No rung is load-bearing at billions-scale: FCM carries the early stock-Android majority, the mesh + WebPush rungs carry more each year as the userbase and de-googled fraction grow. The custodian problem dissolves because the *network* becomes the doorbell and the relays are fungible.

## Wire contract — `kind + address`, generic from line one

The device publishes a list of bells; the sender walks it and rings the first it can speak:

- `fcm:<project_id>:<token>` — **tenant-qualified.** The token is only ringable by a worker holding *that project's* service-account key. Carrying `<project_id>` lets the sender route to the right key. Yours is `fcm:fgtw-90220:<token>`. ~95% of stock devices.
- `up:<url>` — WebPush/UnifiedPush bearer endpoint. `POST <url>`, no key, no tenant. Any relay.
- (future kinds slot in without touching the protocol.)

A device may advertise **several** bells (e.g. `fcm:` *and* `up:`); the sender uses whichever it is equipped for. **The worker/sender never knows or cares which rung a device is on** — it stores `kind + address` and rings. Rungs can be added, swapped, or die (Google deprecating something) without protocol change.

### No tenant registry, anywhere

The endpoint is **self-describing**: it names its own bell and (for FCM) its own tenant. So the sender never consults a list of volunteers or tenants — it reads the bell off the *recipient's* published endpoint and rings it. Consequences:

- **The client carries zero tenants beyond the one(s) its own build ships.** It publishes *its own* fully-qualified bell, nothing else. There is **no baked-in tenant list in `google-services.json`** — that would make you the build-time gatekeeper for every volunteer host, exactly the re-centralization this design removes.
- **The sending worker holds a `project_id → service-account-key` map** for only the tenants *it* volunteers to relay for (yours holds `fgtw-90220`). Live server config (`wrangler secret`), updatable with no rebuild and no client involvement.
- **Cross-tenant reach needs no key-sharing.** A worker can't ring an FCM tenant whose key it lacks — so to wake a device on someone else's tenant, it rings *that device's* `up:` bell instead. FCM stays intra-tenant (your worker rings your users); WebPush is the universal cross-tenant fallback. This is *why* devices publish more than one bell.
- **Volunteers partition naturally** by which devices registered with them — no coordination, no shared list, no broadcast of "here are the hosts." Each device points at its own bell; each host answers only for the devices that chose it. Your FCM instance is simply the first, biggest volunteer.

### Multi-tenant is build-time, not runtime — and mostly moot

An installed APK is bound to the FCM tenant(s) in its `google-services.json` at build time, so a build can't switch to an *un-baked* FCM tenant at runtime. Two softeners:

- A single build **can** carry multiple FCM tenants (multiple `FirebaseApp` instances → one token each → both bells published). "One tenant per build" is a choice, not a constraint.
- The `up:` rung has **no tenant at all** — a device can add/switch/drop WebPush relays purely at runtime, no rebuild. The thing you'd actually want "switch my doorbell host" for is fully fluid on the rung whose users want fluidity; the rigid FCM rung is the one whose users (stock-Android, app-author-runs-the-worker) don't want to move it. A community *fork* that wants its own FCM tenant is already recompiling — it drops in its own `google-services.json` as part of forking, which is the clean outcome, not a burden.

### Bell redundancy & tenant-outage degradation

A build is not "an FCM tenant" — it is a device that publishes *bells*, of which FCM is one kind. So a device **SHOULD publish a fallback `up:` bell** alongside its `fcm:` bell, and the list is ordered by preference (FCM first — cheap, stock; `up:` second — keyless, tenant-independent).

Then an FCM tenant going down is a **degradation, not an outage:**

- **FCM rung dies** for devices on that tenant — they lose the ability to be **woken from deep Doze** via FCM. (Not messaging; not notifications-while-awake — specifically *remote-wake-from-doze*.)
- **`up:` rung is unaffected** (different infra, no Google, no tenant) → senders **fall through to it** → wake notifications keep working.
- **TCP-keepalive + foreground-service floor is unaffected** → any scheduled peer still delivers and notifies; deep-dozed single-`fcm`-bell devices simply wait until they next wake on their own.

So the precise failure statement: **if `fcm:<tenant>` is a device's *only* bell and that tenant goes down, that device loses wake-from-doze notifications until the tenant returns.** The defense is redundant bells per device, **not** switchable tenants — which is exactly why bells are a *list*, and why a later "simplification" back to a single bell value would reintroduce the fragility. You (operator of `fgtw-90220`) are a single point of failure for *that rung*; mitigations in order of effort: (1) every build also ships an `up:` fallback, (2) build carries a second backup FCM tenant, (3) the mesh / peers-are-fgtw rungs where the doorbell isn't Google's at all.

### Registration

- Device publishes its bell(s) to the worker, **device-signed**, keyed to `handle_proof` — the opaque presence key, so the relay/worker holds a `handle_proof → {bells}` map, never the plaintext handle. See [[project_session_registers]] (network vs vault roots stay separate).
- Toggle **off** → `deleteToken()` (FCM) + a worker call to drop the stored bell. Clean exit.
- Registration blob is opaque to a WebPush relay: it learns an endpoint, not who it belongs to — spam-resistant by not knowing who its users are.

## The FCM sender (worker) — v1-direct, empty wake

Replaces `send_fcm_push()`. **No Cloud Function** (the current one is target-less topic-broadcast and its URL was lost in the `/fgtw → fgtw-bootstrap` rename anyway — `wrangler secret list` is empty). Instead:

- Worker signs its own **OAuth2 JWT (RS256 via WebCrypto)** — the worker already does the Ed25519 version of this dance in `webcrypto.rs` — caches the ~1 h access token (per tenant, if it ever relays more than one).
- Reads `<project_id>` and `<token>` off the recipient's `fcm:` endpoint, selects the matching key from its `project_id → key` map, and `POST`s to `fcm.googleapis.com/v1/projects/<project_id>/messages:send` with a **high-priority, empty `data` message**. Empty = no content, no sender → Google sees only *that* a wake happened and when. (Your tenant is `fgtw-90220`; the URL is parameterized, not hard-coded, so a fork's tenant works unchanged.)
- Android side: `PhotonMessagingService.onMessageReceived` fires even from deep Doze → pokes the foreground service → device re-punches + pulls → posts the **local** "New message" notification (generic, handle off the lock screen, per commit 5bcf2bd's wiring).

> **Legacy-key trap:** if the "key you made" was a *legacy FCM server key* (a long string), it is **dead** — Google shut the legacy API down June 2024. The only live path is the service-account JSON → v1 API, which is exactly what this design uses. One Google sign-in total, to the account owning `fgtw-90220`: Project settings → Service accounts → Generate new private key → `wrangler secret put FCM_SERVICE_ACCOUNT`. No new accounts, no Play Store, nothing recurring.

## What stays separate

The existing `peer_updates` topic-broadcast (IP-change cache invalidation, [`peer_updates.rs`](../src/network/peer_updates.rs)) keeps its own job and is **not** conflated with the message doorbell. It is already slated for gossip-subsumption ([peers-are-fgtw.md](peers-are-fgtw.md)); the doorbell does not depend on it and does not extend it.

## Distribution is unaffected

FCM registration checks that **Play Services** is present on the *device* — never where the APK came from. The adb-sideloaded dev APK already carries a working Firebase setup. Self-served APK off your own domain stays exactly as sovereign as today; the Play *Store* only enters if you ever *want* it as a distribution channel (a separate fight, their review policies not FCM). See [[project_identity_storage_model]].

## Build order (deferred until this doc is agreed)

1. **Reachability clock** (client) — the per-conversation last-heard + fast-window + dozed-threshold + debounced state machine. The keystone; testable standalone; nothing else works without it.
2. **`kind + address` endpoint registration** (worker + client) — device-signed publish/update/drop, `handle_proof`-keyed.
3. **v1-direct empty-wake FCM sender** (worker) — first `kind` behind a generic `ring(endpoint)`; live the moment `FCM_SERVICE_ACCOUNT` is set.
4. **UnifiedPush `up:` kind** — checklist for later; slots in behind the same dispatch with zero worker rework.

## Settings surface

One row on the **Notifications** page ([settings-panel.md](settings-panel.md)): honest copy — *"Instant wake-ups via Google's push relay. Google sees when something's waiting for you, never what or from whom."* Off by default (no token fetched, nothing uploaded). Grays out with "no Google services on this device" where Play Services is absent → tier-1 foreground service keeps working. UnifiedPush, when built, lights the same row up for devices that brought their own distributor. Governed by [[feedback_no_time_based_ui]] (event-shown, interaction-cleared) for any wake-driven UI.
