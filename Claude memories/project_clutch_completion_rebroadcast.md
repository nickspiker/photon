---
name: project_clutch_completion_rebroadcast
description: CLUTCH completes crypto-correct but rebroadcasts its proof forever; fixed with a hidden chain-weave probe that seals on first bidirectional ACK
metadata: 
  node_type: memory
  type: project
  originSessionId: 71fe9349-599c-4d2c-9970-3a988ee9a08f
---

CLUTCH reaches matching proofs (crypto done, both sides save Complete state) but the completion handshake never settles: the initiator re-sends its proof every ~30s indefinitely, and the responder — after 5 `ClutchComplete` resends (~15s) — clears its ceremony state and can only answer "Duplicate proof but ceremony cleared — cannot re-send". Root cause: the ceremony state machine is DECOUPLED from the chat data plane — real messages flow + advance the chat hash chain (proven in the Nick↔James log: "fish"/"with" both ways with ACKs) while the ceremony keeps re-announcing, because nothing wires data activity to ceremony completion. Distinct from [[project_clutch_offer_deadlock]] (that's the OFFER stage; this is the COMPLETE/proof stage).

Fix (implemented 2026-07-05, compiles clean, NOT yet device-verified): a hidden **chain-weave probe** — on `ClutchState::Complete`, each device auto-sends one `CHAIN_PROBE_MARKER` chat message that rides the ratchet unchanged but suppresses its UI bubble. Seal (`Contact.chain_woven`) fires once BOTH directions are proven (`their_probe_seen` + `chain_advanced_by_ack`), which zeroes `clutch_proof_resends_left` and flips the top status from "testing · weaving the chain" → "secured". Any real message also seals (belt-and-suspenders). The proof re-arm at photon_app.rs ~8338 is now guarded `if !contact.chain_woven`. New Contact fields are runtime-only (not persisted). One-probe-each-way validates hop 1 only — the msg-2 ratchet desync in [[project_chain_advance_desync]] would slip past it; bump to 2 probes each way if catching that becomes the goal.
