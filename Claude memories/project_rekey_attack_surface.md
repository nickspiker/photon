---
name: project_rekey_attack_surface
description: Re-key/history-injection threat model (docs/rekey-threat-model.md); the first-met device (public_identity) is un-revocable by construction — fix must land WITH device-remove
metadata: 
  node_type: memory
  type: project
  originSessionId: cf9fbed2-aa87-4ebe-b7cf-fca16d308f75
---

Threat model for CLUTCH re-key + friend-history injection, written up 2026-07-09 in docs/rekey-threat-model.md.

Key facts:
- A re-key is triggered implicitly: a peer's ClutchOffer with keys DIFFERENT from what we completed with ⇒ "peer lost chains, re-key" ⇒ nuke chains + zeroize history key. Friction-free by design; that's the surface.
- **Primary boundary holds:** a keyless spoofer can't force a re-key — the offer must be validly signed by a device pubkey we already trust for that friend (`ClutchOfferReceived` gate: `sender_pubkey == contact.public_identity.key`). conversation_token is NOT the secret (derivable from handles).
- **Attacker WITH a trusted device key** (theft/compromise) can: DoS (force re-keys → destroy chains + history key), and INJECT fabricated history (after re-key they're the peer we recover from; rows stored recovered=true, add-only — dedup keeps local on conflict — but NO UI cue). Cannot: read old msgs (forward-secret), impersonate to third parties, or gain access they didn't already have.
- **No fixed random user-only portable secret exists.** identity_seed/vault_seed = deterministic from the handle (ihi::handle_to_hash / handle_seed); handle_proof = public; device_secret = per-device (doesn't move). A stored user secret would NOT fix injection anyway (device theft = secret theft) — only an off-device secret (memorized / PIPE) would.

**RESOLVED 2026-07-09 — revocation now works end to end (R1 device-remove @ ae5f509, R2 fold-respecting trust @ 9360e7f).** Device REMOVE shipped: Settings→Fleet two-tap Remove drives unbind_device + mandatory rotate_fleet_key(survivors); reusable-clean (clean_device_for_reuse = nuke vault + clear session) on Security "Shred" / JOIN "Start fresh". Trust side: knows_device/answerable_pubkeys are FOLD-RESPECTING — once a contact's chain folds once (fleet_folded_once, persisted), trust only current folded members; public_identity loses its pass if the fold excludes it (the removed-first-met = likely-stolen case). Pre-fold = bootstrap; a fold FAILURE never arms the flag (no trust-nobody on a blip). The 3 CLUTCH gates (offer/KEM/complete) call knows_device not the first-met pin (both revokes removed + lets a friend's new device CLUTCH). Freshness is MONOTONIC: current_members_with_ts carries the chain-tip eagle time; the drain adopts only if tip ≥ last-adopted (stale R2 read can't resurrect a removed device), reseeds on shrink before persist. docs/device-lifecycle.md §3 + docs/rekey-threat-model.md updated to SHIPPED. E2E still to run on real devices (removed device stops being answered; friend's 2nd device CLUTCHes; restart persistence).

**MAC-in-ACK idea — UN-MOOTED 2026-07-09 (same day):** the crypto defense against fabricated history — tag every message pair in the ACK with spaghettify(message ‖ secret), recompute on recovery, reject rows that don't match. Was ruled unbuildable (no portable secret existed); the friend-blinded private identity S ([[project_token_private_identity]]) creates exactly the needed secret, so this becomes buildable and complements revocation (cryptographic rejection of injected history, not just access cutoff).

**Decided direction 2026-07-09:** revocation-that-works (fleet allowlist update + key rotation on remove) is THE fix; observability is the only actionable lever TODAY. User chose to build the re-key NOTIFICATION now (event-shown, interaction-cleared per [[feedback_no_time_based_ui]]); recovered-row cue + rate-limit deferred. See [[project_keyring_design]], [[project_history_recovery]], [[project_device_identity_model]].
