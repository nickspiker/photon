---
name: project-fgtw-key-desync
description: fgtw.org bricked all clients 2026-08-14 — desktop deployed the worker with the PRE-rotation key.rs; canonical key lives only on the MacBook
metadata: 
  node_type: memory
  type: project
  originSessionId: f66c6776-85d6-4763-aa73-65fec1c4b3f3
---

**INCIDENT 2026-08-14 (open until redeploy):** every photon client fails attest with "Challenge verification failed - not authentic FGTW".

**What happened:** the FGTW seed identity was ROTATED 2026-07-30 on the MacBook (photon commit 87de9b7 — the old cluster secret was believed lost; it actually still sits in the desktop's gitignored fgtw-bootstrap/src/key.rs, mtime June 9).
Canonical post-rotation keypair lives ONLY at MacBook ~/Code/keys/fgtw-seed-key.rs (0600); client pin = ed25519 021CDF80…, x25519 D60B9DAC….
On 2026-08-14 20:30 local (wrangler version 7db19fbc, the brick deploy) the desktop deployed the worker with its stale pre-rotation key.rs (ed25519 6D9F6E73…, x25519 3D5563A3…), reverting fgtw.org to the rotated-away identity.
Verified live: a challenge fetched from fgtw.org verifies against the desktop key.rs and FAILS the client pin.
Both halves are broken for released clients: challenge signature (ed25519 pin) AND announce encryption (x25519 pin ECDH).

**The "key warning" at deploy was a red herring:** wasm-pack's "License key is set in Cargo.toml but no LICENSE file(s) found" — licensing metadata, not crypto. NOTHING checks key.rs against the client pin at deploy.

**Fix:** redeploy the worker with the canonical key — either deploy from the MacBook, or copy MacBook ~/Code/keys/fgtw-seed-key.rs → desktop fgtw-bootstrap/src/key.rs first. No client change needed.

**How to apply:**
- fgtw-bootstrap/src/key.rs is gitignored and PER-MACHINE — a worker deploy silently ships whatever key that machine holds. Before ANY worker deploy, confirm key.rs public == photon's pinned FGTW_ED25519_PUBLIC_KEY (021CDF80…).
- Add that check to fgtw-bootstrap/deploy.sh (compare both pins, refuse on mismatch) — not yet built.
- After the redeploy, sync key.rs to the desktop so this can't recur.

Relates to [[project_two_machine_git_divergence]] (same two-machine drift class, secrets edition) and [[project_lockout_enforcement]] (the deploy that carried the wrong key).

## RESOLVED 2026-08-14 evening — rollback, guard, custody state

**Un-brick:** `wrangler rollback 75f7f719` restored the 2026-08-12 worker version (canonical key compiled in, no secret needed); live challenge verified against the client pin.
Side effect: the brick/device-lock worker features (1a2afdd) are UN-DEPLOYED until the worker redeploys with the canonical key; v53 lock UI calls will error against the live worker.

**Guard SHIPPED (fgtw-bootstrap bcb830d):** deploy.sh refuses any worker deploy whose src/key.rs publics ≠ photon's pinned constants (both curves), and post-deploy verifies the live challenge embeds the pinned key (probe/challenge-probe.vsf fixture).
Tested: refuses on the desktop (stale key), live check green post-rollback.
Desktop worker deploys are BLOCKED BY DESIGN until the canonical key.rs lands here.

**Key custody (Nick's doctrine: a key not in /mnt/Octopus/Code/keys → Chiton → MEGA does not exist):**
- Verified IN keys/ + mirrored on Chiton: photon-signing-key (byte-identical to AUTHOR_PUBKEY — the update/self-verify trust root), macos codesign, TOKEN.p12, google-services.json, ferros/spirix/ssh/tokens.
- NOT in keys/ ANYWHERE reachable: the canonical FGTW cluster secret (021C…/D60B…). Not on Octopus, not on Chiton, not in Chiton's MEGA Code tree. The rotation saved it to MacBook ~/Code/keys — which keystore.sh says is that machine's MEGA sync dir, yet it never propagated. Chiton keys/ mtime is Jul 26 (pre-rotation): the mirror chain may be stalled or one-way — CHECK MEGA CLOUD (web) for Code/keys/fgtw-seed-key.rs before any drive; also MEGA trash/versions (a one-way Chiton push could have deleted the MacBook's upload).
- Stale pre-rotation pair archived as keys/fgtw-seed-key-STALE-pre-2026-07-30-rotation.rs (clearly labeled, header warning inside).
- TOKEN.p12 password lives only in OS keyrings (GNOME/macOS), not in keys/.

**Paths to full health (either works, no gas money):** (a) canonical secret recovered from MEGA cloud or MacBook → keys/fgtw-seed-key.rs + fgtw-bootstrap/src/key.rs → redeploy brick worker, guard goes green; (b) deliberate re-rotation: mint a fresh pair INTO keys/, repin photon, ship v54 + worker together (auto-update path verified attest-independent: online = bare GET /status, manifest signed by the release key).

## CLOSED 2026-08-14 night — canonical key recovered, brick worker back live

The canonical pair survived in the MacBook's DEPLOY COPY (~/Code/fgtw-bootstrap/src/key.rs, gitignored so no sync touched it); the MacBook's ~/Code/keys/fgtw-seed-key.rs canonical file was GONE — the stalled/one-way MEGA sync ate it.
Recovered via paste, verified all four ways (each secret derives its public, both publics == client pins), installed at /mnt/Octopus/Code/keys/fgtw-seed-key.rs (0600, canonical home) + desktop fgtw-bootstrap/src/key.rs.
Worker redeployed from the desktop (version a51c9194): guard green, brick/device-lock features live again, live challenge signature-verifies against the client pin.

**Residue:**
- Chiton→MEGA mirror STALLED since Jul 26 — until it runs, the canonical key is desktop-only again; kick the mirror and confirm keys/fgtw-seed-key.rs lands on Chiton.
- MacBook key.rs lacks the FGTW_X25519_PUBLIC const — the deploy.sh guard will refuse deploys from the MacBook until the 4-const canonical file is copied over it.
- MacBook unpushed-work sweep suggested but not yet run.
