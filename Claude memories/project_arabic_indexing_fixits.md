---
name: project_arabic_indexing_fixits
description: "fix-it list — decimal/arabic-indexed VSF field names (AGENT.md-forbidden); fix = VSF native multi-value fields (name:v1,v2,v3), NOT numbered names; user said don't fix yet"
metadata: 
  node_type: memory
  type: project
  originSessionId: 77046248-8b61-4358-9ed9-26d48a57df98
---

AGENT.md forbids decimal digits in field names / programmatic counting; the fix for every entry below is VSF's NATIVE multi-value field — `(name:v1,v2,v3)` — one named field, commas are the segmentation (user 2026-07-16: "RÅ<VSF>[(already:has,a,very,nice,way,to,input,multiple.things)]"). Do NOT fix until the user says go; all are flag-day protocol/vault changes (no compat shims, per AGENT.md).

**Violations (photon src/network/fgtw/protocol.rs):**
- `sync_{i}_tok` / `sync_{i}_osc` — pong sync records, encode (~429/433) + parse (~1190). → one multi-value `sync` field of (hb tok, e6 osc) pairs.
- `peer_{i}` prefixes (~261) — PhonebookResponse peer rows. → repeated multi-value `peer` field (the worker mirrors this parse — fgtw-bootstrap/toka change too).
- `device_{i}` prefixes (~312, ~376) — device-list messages. → repeated multi-value `device` field.

**Violations (photon profile expandables, src/ui/photon_app.rs + docs/contact-system.md):**
- `profile.addr2` / `email2` / `phone3`… settings keys + `profile.<id>_label` companions (my 2026-07-15 expandable-fields work — caught by the user). → ONE key per base (`profile.addr`), value = multi-value rows; instance identity = the TAG (home/work/custom, free text), never a number. Every multi-instance field gets a tag box (not just phone); first instance may be untagged, later ones need distinct tags. Trade-off accepted pending review: per-base (not per-instance) LWW on sibling concurrent edits. Retire the `addr2`/`email2` dictionary labels from the doc taxonomy.

**x-for-native-type squatters (same doctrine — VSF has the type, the code stores a string; found only after ACTUALLY reading src/types/vsf_type.rs, 2026-07-16):**
- contact_state `ip` — `ip.to_string()` stored as AnyString, `parse()`d back (src/storage/contacts.rs ~161/203/380). → `ns` (host+port) or `ni`/`nj`+`np`.
- Structured-log rework (numbers-binary-at-rest, see [[feedback_numbers_binary_at_rest]]): log `msg` strings bake base-10 values; fix = typed named fields beside pure-text msg, delete `deglyph_for_log`, photonlog renders dozenal words at read time.
- friendship.rs ~287/388 read `x` values as raw bytes (`s.as_bytes().to_vec()`) — inspect what's actually stored there; bytes belong in a bytes/hash/tensor type, not text.

**Worker/R2 (fgtw-bootstrap), from the 2026-07-16 full numerics-as-text screen:**
- R2 object keys bake numbers as decimal text: `relay/{hex}/{seconds:.6}.vsf` (a FLOAT in a storage key) and log keys `{osc}-dev{hex}.vsf`. Keys must be strings (external constraint) but the encoding is ours → fixed-width hex of the binary i64 (base-honest + correct lexicographic sort).
- `log_list` ack returns all object keys newline-JOINED in one string field (blob.rs ~369 reads it) → native multi-value field, one key per value.

**Verified CLEAN in the same screen (don't re-audit):** settings knobs (u3 / raw bytes), fstate codec, phonebook peer ip (binary vector), pong observed_addr, manifest v2, JNI numerics, fgtw crate codecs. Judgement-call exemptions: .sha256 sidecars (PowerShell Get-FileHash interop), user-TYPED profile values (prose).

**Minor / borderline:**
- `kind_{other}` fallback field name (src/network/fgtw/blob.rs ~470) — decimal-derived name from a numeric kind; rename to a self-describing form when touched.
- `friend{hp}` (fleet.rs / fgtw fstate.rs) — content-derived (handle-proof hex), NOT counting; excluded.

Related: [[project_vsf_canonical_signing]]; the contacts-codec positional-parse note lives in the 2026-07-15 audit (bare-section-vs-complete-file decision also still open with the user).

**Worker register sweep DONE 2026-08-15 (fgtw-bootstrap 7e93e97):** every R2 register value is now a typed VSF doc (ts registers ride creation_time e6; fanout epoch = u field; device_lock hp; auth slots ke), legacy raw fallbacks on read until entries migrate on next write. REMAINING raw surfaces, flagged not fixed: peer_count.bin (variable-width count) + the inbox .rec composite records (kind+dev+by+t_osc) — convert when their features are next open.

**vsf builder l-field bug (found by keteinfo on a live vault 2026-08-16):** documents built via VsfBuilder + add_unboxed stamp `l` (file length) as the HEADER length only — inspector flags "l3(129) vs actual 1646" on every fstate doc. Parsers tolerate it; fix in vsf's build() with the frozen-wire pins updated deliberately, not mid-session.
