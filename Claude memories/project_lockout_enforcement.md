---
name: project_lockout_enforcement
description: Fleet lockout was one-directional (others refuse a device, device never self-enforced). SHIPPED 2026-08-14 @b75cc0e: presence-gate + self-lock-go-dark + mesh identity authority. Focus bug also fixed @fluor dcde767.
metadata:
  type: project
---

Field report: "this machine should be locked out but friends still show online." Root cause = fleet lockout was ONE-DIRECTIONAL by design (peers refuse the locked device; the device never checked its OWN key against fleet.locked and kept broadcasting presence). Three gaps closed, SHIPPED @b75cc0e 2026-08-14 (photon):

1. **Presence-ping gate leaked.** `Contact::answerable_pubkeys()` (the flat set the RAW ping RX gate at status.rs ~2314 checks) did NOT exclude `locked_out` (whole-contact bool) or `refused_devices` (per-device) — only the downstream `knows_device`-gated chain/CLUTCH handlers did. So a locked device's PINGS were answered. Fixed: answerable_pubkeys drops locked_out + refused; `apply_locked_set` now calls `reseed_contact_pubkeys()` immediately on lock (it didn't). Friend-side refused_devices applies at next reseed (~45s) not instantly — mid-pong-loop reseed was too invasive, and that device's frames are already chain-blocked.

2. **No self-enforcement (the real "should be locked but online" fix).** `apply_locked_set` now checks if OUR OWN device key is in the fleet.locked set; if so, clears the session (`tohu::clear_session()`, session=None, drop to `AppState::Launch`), so presence stops (it's state-gated to Ready/Conversation) and the machine goes dark. Resume is handle-gated (owner re-attests, thief without handle can't). Runs on the sync tick, so it fires within a tick of the locked-set syncing. NOTE: running a build with this on a machine already in fleet.locked will immediately go dark — that's correct.

3. **Mesh peer-store could seed public_identity with an impostor** (the "thief's device answered as you" threat, handle assumed known). `search_with_refresh` (handle_query.rs) consulted the unverified local peer store BEFORE the signed fold, so a self-signed PeerRecord under a SCRAPED handle_proof (anyone who knows the handle can compute it) could win over the chain and become a contact's `public_identity`. Fixed: signed fold (`current_members_full`) is now AUTHORITATIVE for public_identity; the store is a fallback ONLY when the chain is unreachable (availability), and the genesis-pin (photon_app.rs ~10914) unmasks any impostor on the first successful fold, bounding exposure to the pre-fold window. public_identity is NOT clutch-derived — it's members[0] from the fold; a thief can't forge fold membership with only a handle (needs identity_seed for genesis or a member's device key + consent_sig for bilateral add).

STILL OPEN (deliberately not done): self-lock uses reversible clear_session (handle re-unlocks) — fine for the threat model. The deeper mesh membership gate (pass folded known_fleet to merge_peer on gossip ingest) is tracked in docs/peers-are-fgtw.md; merge_peer_bound already supports it, production gossip ingest wasn't wired to a contact-seeding path in this pass. See [[project_device_sovereignty]] (self-signed removal only), [[project_rekey_attack_surface]], [[project_peers_are_fgtw]].

ALSO fixed same session: the desktop window FOCUS bug (app didn't come topmost on launch/click, sometimes went backwards) — SHIPPED @fluor dcde767: explicit focus_window() on first launch (skipped when start_hidden) + raise-on-click when !is_focused. Root cause = borderless monitor-sized transparent surface that Linux WMs with focus-stealing-prevention decline to auto-raise, and no explicit activate anywhere. Field-verified on Muffin.

## Worker-authoritative brick SHIPPED 2026-08-14 (3-repo)

The self-lock (gap 2) reads the LOCAL fleet.locked cache, which a WIPE erases — so a wiped+reattested stolen device reached contacts. The worker is the one authority a wipe can't touch. SHIPPED: worker fgtw-bootstrap @1a2afdd, client fgtw @5b3dd39, photon @8e02ac4.

- WORKER (fgtw-bootstrap/src/lib.rs): device_lock/ R2 index (mirrors device_owner/); handle_announce refuses a device whose pubkey is locked by its own handle_proof (returns reason "device_locked"). device_lock/device_unlock endpoints clone handle_device_release's auth verbatim (Ed25519 sig + fleet-fold membership gate) — only a CURRENT member of that exact chain can lock/unlock, airtight vs lock-DoS. UNLIKE release, locking a current member is ALLOWED (a stolen device stays a member; removal is self-signed-only). DEPLOYED 2026-08-14 (worker version 7db19fbc) — enforcement is LIVE on fgtw.org.
- CLIENT (fgtw/src/client.rs): device_lock/device_unlock off device_release; photon wrappers fleet::lock_device/unlock_device.
- PHOTON: lock_out_device pushes to worker (spawn_worker_lock_push) + reconcile_worker_locks re-pushes all locked devices on attest-success (idempotent, converges a failed initial push). Worker's device_locked announce refusal -> QueryResult::Locked (stable "device_locked" reason prefix in bootstrap.rs reason_error) -> LaunchState::Locked (can_edit_handle=false) -> dead-end red screen (textbox/infinity/Attest suppressed), session cleared, binds/persists NOTHING so it re-appears every attest wipe-or-not.

OPEN — UNLOCK UI not built (deliberate): locked_devices() is a grow-only pubkey_set_union with the comment "an unlock is a deliberate future flow, not a merge artifact". A correct unlock needs TIMESTAMPED LWW (lock@t1 vs unlock@t2 latest-wins), NOT set subtraction — else lock->unlock->relock reads as unlocked. The worker device_unlock endpoint EXISTS; the photon side needs: LWW tombstone in settings + clear contact.locked_out + rebuild relay/pong report + reseed answerable pubkeys + call fleet::unlock_device. Until built, a mis-lock is recoverable only via a manual worker device_unlock call.

REBASE TRAP hit this build: fgtw-bootstrap was 12 commits behind (another machine); the remote had refactored announce (peers.vsf -> per-device ann/ marker), so my lock-gate insertion conflicted and my base's peers.vsf code was stale. Resolved by keeping remote's ann/ marker + my lock gate, then RE-VERIFYING compile against the current base (the sibling-staleness lesson: always re-check after rebasing onto a diverged remote).

## UNLOCK SHIPPED 2026-08-15 (photon 0f76044 + worker fa9e765, deployed 9f2f8a81)

Worker: lock/unlock refuse a LOCKED signer (an un-wiped stolen device could self-unlock or spite-lock the fleet — the member gate alone allowed it) + MONOTONIC OP GUARD (601bc76, deployed 00b61d1a, replacing the brief ±10min window Nick vetoed): lock_seq/<device> = last executed stamp (eagle osc, 8B LE), frames execute only strictly-newer — replay AND held-first-delivery both die on order, no wall clock; future cap (2min skew) only guards hwm poisoning, nothing honest can die on it.
Photon: Unlock pill mirrors the lock (two-tap → de-attest → fires inside the handle-confirmed attest via pending_unlock); handle NEVER prompted on the locked device itself (phishing doctrine) — its dead-end gains a retry pill back to the normal resume entry.
Unlock = value-level tombstone (EMPTY fleet.locked.<hex> value; pubkey_set_union drops len!=32 on every build) + legacy-blob rewrite + worker push; reconcile_worker_unlocks re-drives on attest-success (inverse of reconcile_worker_locks) — self-healing against failed pushes AND sibling stale-lock re-pushes.
apply_locked_set clears locked_out ONLY on an affirmative tombstone (absence = boot-lag, never forgiveness).
Fixed alongside: compliance rotation re-wrapped locked devices (unfiltered members) — any new-egg rotation undid the lock's key rotation; now filtered like the heal. Unlock re-admission = same call (grow mints the next epoch with the device back in).
Last-other-device lock warns at confirmation (locked+lost-sibling = permanent brick until custodian supersession).
E2E pending: lock→unlock→retry round trip on real devices.

**Field find 2026-08-15 (fixed @6217eb6):** the launch textbox retains its text, so every handle-re-proof gate AUTO-POPULATED its own answer — unlock/lock confirmations, Security self-lock, go-dark, Locked verdict, []u. clear_handle_for_reproof wipes the field at all six; convenience retention (attest-error retype) untouched.
