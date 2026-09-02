---
name: project_self_message_vanish
description: "self/fleet messages vanish on restart (is_sibling display rows never persisted) + never-ACK (phone stale/Pending/unreachable) — diagnosed 2026-09-02, fix NOT started"
metadata: 
  node_type: memory
  type: project
  originSessionId: 4dcd0cb3-95b3-4d99-850b-f0d40f6ad308
---

Field 2026-09-02 (Emma+Nick calls failing, self-messages vanishing twice desktop AND android). THREE stacked faults, diagnosed from both sides' logs:

**1. VANISH (real code bug, platform-independent).** The self/fleet conversation is stored as per-sibling tables, each flagged `is_sibling`. [messaging.rs:185](src/ui/photon_app/messaging.rs) `persist_messages_signalled` returns early WITHOUT persisting for any `is_sibling` contact ("BRIDGE rows are EPHEMERAL"). That ephemeral gate was built for `$ ` bridge-terminal commands (2026-08-22 braid break) but it also eats ordinary FLEET CHAT rows → they live in RAM only → gone on restart. Vanished on BOTH desktop + android = confirms shared-code, not stale-build. FIX (not started, Nick hasn't greenlit): `is_sibling` is per-contact but "ephemeral" is per-MESSAGE — persist sibling conversations, filter OUT only the `$ `-prefixed bridge rows at save time; leave braid/chain (anchor-only sibling frames) untouched (durability lives separately in friendship_chains, so display-table persist can't re-break the braid). Add round-trip test: fleet row survives reload, `$ ` bridge row doesn't.

**2. NEVER-ACK (~20 both sides).** Phone `cacbc223`/`1be949c1` is the wall: APK built 15:26 but beacon fix [[project_render_storm_lag]]... actually fix 7e6ce23 landed 15:40 (14min later) → phone runs pre-fix code; desktop hears ZERO LAN beacons back + relay-drops to `1be949c1` 50× (pipe closed); fleet pairing to phone is `Pending` in census (ceremony never completed). So self→phone can't deliver → never ACK; desktop re-serves the same row forever ("Skipping duplicate ... no stored ack_hash"). Needs phone rebuild+reinstall (scripts/android/dev-adb.sh) — necessary but does NOT fix the vanish. Related identity-era split [[project_call_no_ring_incident]].

**3. VAULT 5s SLOW puts.** kete puts up to 5332ms blocking every vault caller — perf regression on the group-commit path [[project_vault_op_latency]], compounds lag, can guillotine a send if app closes mid-put.

Also this session: PT SPEC retry ladder tuned to ms (200ms→3.2s) shipped 5e418f5. Call glare: Emma+Nick dialed simultaneously → each auto-BUSY'd the other's offer, no ring, no missed-call notification; offer fires to only 1 (stale) path instead of fanning the fleet.
