---
name: feedback_numbers_binary_at_rest
description: "THE number doctrine: numbers are BINARY at rest — wire, vault, logs, manifests; a display base (dozenal) is chosen only at the render edge; the rule is AGENT.md's PRINCIPLE, not its examples"
metadata:
  type: feedback
---

AGENT.md's "Decimal/Arabic Indexing is FORBIDDEN" is not a lint about `s{idx}_` field names — its first rationale line ("CPUs count in binary") IS the rule (2026-07-16, Nick: "Is it a reading problem?" after three rounds of arabic resurfacing).

**Why:** a number rendered into text at WRITE time (any base — "34" or dozenal words) welds a display decision into the data forever. Binary at rest keeps the base the READER's choice.

**How to apply:**
- Wire/vault/manifest — ABSOLUTE ("if there is any base coded shit saved in VSF other than binary, we gunna have a problem"): VSF native numeric types (`u`, `z`, `e6`, `ni`/`np`/`ns`) — never digit strings or numbers interpolated into `x` text.
- Display is deliberately narrow dozenal: version (+ reputation) in dozenal glyphs; unit quantities, step counters and time stay arabic until humans catch up ([[project_numeral_forms]]). Never a "(dozenal …)" caption. Read-aloud = `dozenal_words` camelCase ([[feedback_voca_camelcase]]). Exempt: dev zoom % watermark, identifier names (x86_64, Ed25519), user-TYPED values, .sha256 sidecars.
- Logs are the standing violation (`format!` bakes base-10 in); real fix = structured log records with typed VSF fields, base rendered at read time ([[project_arabic_indexing_fixits]]).
- Any `format!` writing a number to UI, storage or wire: stop — it's a typed field (storage/wire) or a glyph render (UI).
