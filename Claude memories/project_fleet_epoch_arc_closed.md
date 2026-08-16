---
name: project-fleet-epoch-arc-closed
description: Epoch arc CLOSED @ fa3a9c0 2026-08-16 - hist_page+pong epoch re-seal, row-cadence mint; CAUGHT field_u64 exact-variant bug that had chain_sync + ALL ckpt frames parse-dead since B3
metadata:
  type: project
---

**SHIPPED @ fa3a9c0 2026-08-16** — the fleet-epoch B-arc residue closed, field verify pending:
- **hist_page re-seal**: fleet-route pages seal under `fleet_epoch_seal_key(epoch, b"hist_page")`, `ek` on the frame (OPTIONAL: absent = friend-route under the friendship history key). Receiver opens at k / k−1; behind → ckpt_req + drop (self-heals); sibling page without ek = flag-day drop. No spine = hold (the chain_sync rule).
- **pong re-seal**: sibling pong tails key off the epoch spine (`photon.pong.seal.sib.v1`); map reseeds on EVERY spine edge (mint / root hand-off / state adopt / vault load). Pre-spine = v0 static fallback.
- **row-cadence mint**: 128 fresh syncable rows → ckpt_mint_due in the fleet sweep (cheap len() sum vs base). Won fanout rotation also mints (PCS boundary). Before this the ONLY steady-state mint edge was fleet-join — the spine never advanced.

**THE PARSE BUG (4th victim of the silent-VSF-rejection class, after [[vsf-toc-section-name-trap]]):** `field_u64` matched `VsfType::u(n, false)` exactly, but auto-sized `u` DECODES as the smallest concrete width (u3/u4/…) — the match NEVER fired on a wire frame. chain_sync + ckpt_root + ckpt_req + ckpt_state all failed parse silently since B3 shipped: fleet chain replication and the device-to-device spine plane were DEAD in the field (the wiped mac "holding for ckpt_state" forever while the desktop served it = this). Fixed with `as_u64()`; round-trip tests over real encode+decode now cover every integer-carrying sibling frame. **RULE: never variant-match a parsed VSF integer — always `as_u64()`/`as_usize()`; and every new frame gets a build→parse round-trip test.**

**Field soak round 1 (2026-08-16 evening) found TWO wedges, both FIXED @ 2a2d267:** (1) spineless hold was permanent — dead k=1 custody (minter wiped, fleet key rotated past the seal) + B-arc holds parked ALL fleet traffic behind the missing spine; now ckpt_req per hold sweep + 3-strike SUPERSESSION (fresh seed at chain_k+1, worker CAS one winner, custody under current fleet key). (2) friend refused_devices had NO reversal — the lockout E2E left the desktop permanently deaf at Emma; pong locked-set gossip now retracts per-reporter (absent device = retraction, zero reporters = un-refused, empty set processed). Round 2 soak pending on ≥2a2d267 everywhere INCLUDING Emma's phone (the refuser must run the un-refuse code).

Desync-repro audit (2026-08-16): the pinned cross-probe mechanisms ([[project-chain-advance-desync]]) are addressed in code — probe rows persist for re-ACK durability, are weave-ineligible (the fork vector), nonce fork-repair is the backstop. The LIVE field breakage was the parse-dead chain_sync all along. Two-device soak on ≥fa3a9c0 builds is the confirmation gate for clearing that memory.
