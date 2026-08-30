---
name: project-zero-sentinel-purge
description: zero-sentinel purge SHIPPED f1d28b3 (Option device keys + local_ip); RELAY_ADDR/RosterEntry/ACK-API still carry sentinels — follow-up list
metadata: 
  node_type: memory
  type: project
  originSessionId: 9486060b-4dcb-4d7e-be00-c2a128f2d9f5
---

Doctrine (Nick, 2026-08-30): absence is `Option`/omitted-VSF-field, never an in-band zero.
SHIPPED f1d28b3: Contact.public_identity + CloudContact.device_pubkey are Option; keyless ContactId keys on party_id (was blake3(zeros) — every stateless contact collided); presence pings / offers / knocks / blind frames / avatar fetches skip keyless contacts instead of addressing device 00000000; vault + cloud blobs omit the absent key; get_local_ip failure no longer mints 0.0.0.0:port.

**Why:** the sentinel escaped containment repeatedly (42,820 retransmits to 0.0.0.0:0; signed zero-address peer records circulating forever; "pinged 00000000 → marked offline") and each leak needed its own guard.

**How to apply — remaining sentinel carriers (convert when touched):** RELAY_ADDR (0.0.0.0:0) in the send/dispatch structs (deliberate protocol tag, guards at every ingest — Option-izing the request structs is its own design block); fgtw RosterEntry.public_identity wire slot (zero = keyless, decoded to None at the one read boundary in devices.rs); the ACK/re-ACK request path's `unwrap_or([0u8;32])` recipient in conversation.rs (unreachable in practice, annotated); CLUTCH response arms use device_key().unwrap_or_default() with "unreachable-zero" comments pending request-API Option-ization. Ceremony instance arbitration + jittered yield landed beside it (054077d).
