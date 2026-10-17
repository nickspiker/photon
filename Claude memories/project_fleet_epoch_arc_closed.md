---
name: project-fleet-epoch-arc-closed
description: Epoch arc CLOSED @ fa3a9c0 2026-08-16 — hist_page + pong epoch re-seal, row-cadence checkpoint mint; field_u64 exact-variant bug had chain_sync + all ckpt frames parse-dead; ten-round soak healed lanes (implied-ACK, tip-clear, stuck-tip rotation)
metadata:
  type: project
---

**Shipped @ fa3a9c0 (2026-08-16):**
- hist_page fleet-route pages seal under `fleet_epoch_seal_key(epoch, b"hist_page")` with `ek` on the frame; receivers open at k / k−1, behind → ckpt_req + drop.
- Sibling pong tails key off the epoch spine (`photon.pong.seal.sib.v1`), reseeded on every spine edge.
- Row-cadence mint: 128 fresh syncable rows → `ckpt_mint_due`; a won fanout rotation also mints.

**THE PARSE BUG:** `field_u64` matched `VsfType::u(n, false)` exactly, but auto-sized `u` decodes as the smallest concrete width — chain_sync and every ckpt frame failed parse silently since B3, so fleet chain replication was DEAD in the field.
**RULE:** never variant-match a parsed VSF integer — use `as_u64()` / `as_usize()` ([[feedback_vsf_readers_width_agnostic]]); every new frame gets a build→parse round-trip test.

**Ten-round soak (2026-08-16 → 08-18) left these heals:** spineless hold → ckpt_req + 3-strike supersession; orphaned refusals (zero standing reporters) yield to a clean trusted pong; a relay sentinel / flapping ip never counts as "has an address" (resolve gate = `validated_path.is_none()`); resolve walk uses a rotating cursor; in-order lanes imply-deliver everything older than an acked row; the peer's sync-record lane tip clears pendings; send attempts persist across restarts; pong salvage passes sync records; stuck-tip rotation after 2 exhaust→re-arm ladders at one head.
Residual relay tier between bridged subnets was the router (inter-segment unicast filtered), not protocol. Related: [[project-chain-advance-desync]], [[reference-vsf-primary-section]].
