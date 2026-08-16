---
name: feedback_numbers_binary_at_rest
description: "THE number doctrine (AGENT.md's real rule): numbers are BINARY at rest — wire, vault, logs, manifests; a display base (dozenal) is chosen only at the render edge. Arabic digits never in UI text, stored strings, or field names; the rule is the PRINCIPLE, not its three examples"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 53d3525d-cffb-46fd-994e-0fa79c461c42
---

AGENT.md's "Decimal/Arabic Indexing is FORBIDDEN" is not a serialization lint about `s{idx}_` field names — its first rationale line ("CPUs count in binary") IS the rule, and I repeatedly violated it by reading only the examples (2026-07-16, user: "Is it a reading problem?" after three rounds of arabic resurfacing).

**Why:** a number rendered into text at WRITE time (any base — arabic "34" or dozenal words "ZilorStela") welds a display decision into the data forever and destroys machine-readability. Binary at rest keeps the base the READER's choice.

**How to apply:**
- Wire/vault/manifest — AT REST IS ABSOLUTE (user 2026-07-16: "if there is any base coded shit saved in VSF other than binary, we gunna have a problem"): VSF native numeric types (`u`, `z`, `e6`, `ni`/`np`/`ns` for addresses) — never digit strings, never numbers interpolated into `x` text values, no base-coded anything.
- DISPLAY is deliberately narrow dozenal (settled 2026-07-16): the acclimation surfaces are VERSION (+ REPUTATION when it lands) in dozenal glyphs; unit quantities (KiB/MiB/hours/%), step counters, and TIME stay current mixed arabic units until humans catch up. NEVER a "(dozenal …)" label — glyphs need no caption. Read-aloud/spell = dozenal words (`dozenal_spell`/`dozenal_words` camelCase per [[feedback_voca_camelcase]]). Exempt: dev-only zoom % watermark; identifier NAMES (x86_64, Ed25519, BT.2020) are nouns; user-TYPED values stay as typed; .sha256 sidecars stay (Windows interop, binaries self-verify).
- Logs: the standing violation — `crate::log(&format!(...))` bakes base-10 into ~every message; `deglyph_for_log` was the same sin in dozenal. Real fix (fix-it list, batched with [[project_arabic_indexing_fixits]]): structured log records — msg stays pure text, values ride as typed named VSF fields; photonlog/UI render the base at read time.
- Every `format!` writing to UI, storage, or wire: if a number is being interpolated into text, stop — it's either a typed field (storage/wire) or a glyph render (UI).
