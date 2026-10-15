---
name: project_address_ports
description: 2026-10-15 address records carried ONE port — LAN candidates used the NAT's public port; two devices could share one public ip:port; PT hammered unproven addrs. Fixed + server deployed; field verify pending
metadata:
  type: project
---

2026-10-15 CONVICTED from Nick's phone log (Jon = contact aefd768a, two devices 7d23ad30 + b94e4b48 both adopted 174.45.252.101:49189, LAN 192.168.1.66:49189 / .10:49189):
1. Phonebook records had ONE port (the reflexive/NAT public one); every LAN candidate was local_ip + public port → same-LAN direct could never connect (protocol.rs harvest, phonebook_client::record_local_addr, status.rs relay-ping punch, launch.rs add-friend).
2. Two devices of one contact could hold the same public ip:port (one stale) → punch device A at device B's mapping.
3. PT retried an unproven record address for minutes (3,595 transfers to 97.186.75.40:4383) while the punch had parked on "pending relay"; that IP hosts several devices (ports 4383/44306/22337), owner of :4383 not attributed.

FIX (photon 2026-10-15, fgtw + server fed95582): records state the LAN listen port — gossip PeerRecord.local_port at field index 7, registry DA_LOCAL_PORT bytes 139..141, both SIGNED ONLY WHEN NONZERO so old records verify unchanged; LAN candidate = lan_port() (stated, else PHOTON_PORT). adopt_endpoint_into: newer claim (seen) keeps a shared public; older cleared/skipped. Contact::direct_parked → chat + molecule routes relay-only.

ROLLOUT ORDER: the FGTW server must verify the new field BEFORE clients publish it (old server rejects nonzero-local_port records as bad_signature). Server deployed first. Old clients drop new records until updated (flag day). toka needed vsf "0.10" bump for the server build.
OPEN: flapping friend (green↔amber) unconfirmed — want both logs during a flap; Jon's log for which device owns 49189 / whether his network has IPv6.
