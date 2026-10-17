---
name: project_traversal_relay_gap
description: "Traversal state: punch pulse for relay-only friends, amber ring, NAT-PMP/PCP/UPnP portmap, relay-nudged open, adopt_endpoint_into reconciler BUILT; relay media + symmetric-NAT prediction unbuilt"
metadata:
  type: project
---

**2026-09-17 conviction (Jon's Mac, home NAT ↔ Nick's phone, cellular CGNAT):** three waves signalled over the relay and carried zero media.
Fixed: any online contact with no validated path keeps the resolve+punch pulse alive; `contact_conn_tier` shows amber (not green) without a validated path; the wave pill reads "no direct path".

**Built 2026-09-17:**
- `src/network/traverse/portmap.rs`: NAT-PMP → PCP MAP → UPnP IGD, per network epoch (`start_portmap`), posts the granted address thru `post_reflexive_seed`, renews at half-life.
- Relay-coordinated open (`relay_nudged`): a ping arriving via the relay fires probes at the sender's record immediately, inside the window its own punch opened.
- `reseed_reflexive_from_fgtw` logs start and names why it bails.

**2026-10-01 stale-address flap (Theresa's dynamic home IP):** the harvest adopted only for contacts with no address. Fix: `peers::adopt_endpoint_into` is THE one reconciler (change-edge, skip-validated, locked-device guard); any adoption resets backoff and pings = re-punch both ways. Field verify pending.

**Still missing:** relay media (engine send down the pipe when no path validates); symmetric-NAT port prediction; publishing our own global IPv6 host.
Nick's stance: avoid relay media (cost, metadata, latency) — exhaust direct tricks first.
Related: [[project_address_ports]], [[relay-asymmetry-ping-reflection]], [[project_call_no_ring_incident]].
