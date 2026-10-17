---
name: project_token_private_identity
description: "TOKEN crux SOLVED 2026-07-09: private root S survives via friend-held one-time-pad blinds (S ⊕ pad(device, friend)); fleet weave + S lifecycle shipped; MAC-in-ACK not built"
metadata:
  type: project
---

The TOKEN crux — a private identity held by no other party in any form, yet survivable with no central store — resolved 2026-07-09 (Nick).

**Construction:**
- S = random user secret minted at first-device genesis. NEVER handle-derived; the handle-deterministic layer stays the PUBLIC rendezvous layer.
- pad(D,F) = PIPE_D("secret" ‖ handle ‖ friend) — lossy OWF over the device secret, recomputable, never stored.
- blind(D,F) = S ⊕ pad(D,F), deposited with friend F. Full-entropy pad ⇒ one-time pad ⇒ friends provably hold "not the secret in any form".
- Recovery: a friend serves the blind ONLY to a currently-attested fleet member; S is hot in RAM briefly, zeroized, never at rest.
- Per-device AND per-friend pads; a new device gets S once over the sealed pairing channel at ADD.
- Theft: friends refuse the serve to a refused/locked device (`refused_devices`, effective_trust = member AND NOT refused — shipped, see [[self-only-removal]]).

**Don't-break list:** wrap the blind in AEAD keyed by the pad + cross-check against ≥2 friends (raw XOR is malleable); S never handle-derivable; hot-RAM discipline ([[project_secret_memory_hygiene]]); keep the handle out of the pad input or re-deposit at rename.

**Built 2026-07-09:** phase 1 fleet weave = full CLUTCH between own devices (synthetic is_sibling contacts keyed by `sibling_party_id`, fc841f4); phase 2 S lifecycle (`src/crypto/blind.rs`, blind_put/ack/get/srv frames, probe-before-generate, Provisional→Live only after the friend's disk commit; S transfer between siblings is NEVER a chat message, which would put S at rest; f057c5d).
**Not built:** phase 3 MAC-in-ACK (tag = spaghettify(message ‖ S) so injected history fails recompute).

**Endgame pointer:** with S as root the handle becomes a mutable label (rename by S/device continuity, no public old→new link); friendship_id / conversation_token / vault roots should eventually anchor on an S-derived key.
Design of record: docs/device-trust-and-recovery.md. Related: [[project_total_loss_recovery]], [[project_token_terminology]], [[project_keyring_design]].
