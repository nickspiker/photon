---
name: project_arabic_indexing_fixits
description: "FIX-IT list (don't fix until Nick says go): decimal-indexed VSF field names and numbers-as-text → VSF native multi-value fields (name:v1,v2,v3), never numbered names"
metadata:
  type: project
---

AGENT.md forbids decimal digits in field names; the fix for each entry is VSF's NATIVE multi-value field — one named field, commas segment (Nick 2026-07-16).
Do NOT fix until Nick says go; each is a flag-day wire/vault change (no compat shims).

**Still open (checked 2026-10-17):**
- Profile expandables `profile.addr2` / `email3` … + `profile.<id>_label` (settings.rs `sync_expandable_fields`) → ONE key per base, value = multi-value rows; instance identity = its TAG (home/work/free text), never a number; every multi-instance field gets a tag box.
- Log `msg` strings bake base-10 values → typed named fields beside a pure-text msg, delete `deglyph_for_log`, photonlog renders at read time ([[feedback_numbers_binary_at_rest]]).
- storage/friendship.rs reads some `x` values as raw bytes — bytes belong in a bytes/hash type, not text (inspect when touched).
- fgtw-bootstrap: R2 log keys bake decimal text (`{ts}-dev{tag}`) → fixed-width hex of the binary i64; the `log_list` ack joins keys with newlines in one field → multi-value; `peer_count.bin` variable-width count + inbox `.rec` composite records → typed VSF when next touched.
- `kind_{other}` fallback field name (fgtw/blob.rs) → a self-describing form when touched.
- vsf builder: VsfBuilder + add_unboxed stamps `l` as the HEADER length only (keteinfo: "l3(129) vs actual 1646"); fix in vsf's build() with the frozen-wire pins updated deliberately.

**Done:** protocol.rs `sync_{i}` / `peer_{i}` / `device_{i}` (gone from src), contact `ip` (binary socketaddr), worker register sweep (fgtw-bootstrap 7e93e97).
**Exempt:** `friend{hp}` (content-derived, not counting), .sha256 sidecars, user-typed profile values.
