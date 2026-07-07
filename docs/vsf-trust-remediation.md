# VSF trust-boundary remediation

## STATUS 2026-07-06 — network-facing conversion LANDED (photon b0f18c3, worker 5a35e54 deployed, vsf 15a27c3)

- vsf crate: `read_verified`/`parse_document` are the un-skippable front doors; vsfinfo prints per-anchor verdicts.
- **Signature-scheme fork found and closed**: two schemes coexisted — canonical (ge over BLAKE3(file, ge zeroed): sign_file/verify_file_signature) vs hand-rolled (ge over the bare hp value: worker challenge, photon avatar content doc, relay/fetch requests). Relay was silently dead: scheme-2 TX vs scheme-1 worker verify = every relay send rejected. All signers/verifiers now canonical.
- Converted: avatar.rs (readers + both writers), bootstrap.rs (challenge pinned to the FGTW key, peer list via parse_document), protocol.rs offer parsers, relay.rs signer, worker (challenge, inner-avatar verify, read_verified with pinned signer, skip-walk deleted, stored avatars carry hb).
- `FoldError::Empty` → Fresh/ours at probe + attest verdict (the "chain unverifiable: Empty" fix).
- **LOCKSTEP**: worker deployed — old photon builds fail challenge verification until rebuilt; pre-existing stored avatars (hp-only, scheme-2) are rejected by new readers → re-set avatars once.
- REMAINING: ping/pong/chat/KEM/complete frames use hp as an application field (chain linkage / ceremony_id) — annotated spec deviation, resolves in the messaging rework; vault parsers (debt register below); fgtw crate client reads; the dev.sh hand-roll gate (Phase C).

Photon hand-rolls VSF parse/encode at ~495 sites vs ~16 using the schema API — the "string-concatenated SQL" problem, at trust boundaries.
This doc is the audit synthesis + the fix plan. Source: the vsf-trust-audit workflow (4 parallel readers, 2026-07-06).

## The core defect

The FGTW worker now answers **every failure as a VSF `error` frame `{reason, detail}` at HTTP 200** (fgtw commit 6b01e46).
The `fgtw::client` crate branches on it (`error_frame()`/`is_error()`); photon's DIRECT reads never adopted it and still gate on `response.status()`.

**Critical fact:** a well-formed worker error frame **PASSES `vsf::verification::is_original`** — it's a valid provenance-hashed VSF doc, just carrying an error, not a payload.
So `is_original` can NEVER be the sole gate. Error-frame rejection must be a **separate, explicit, FIRST** step at every FGTW read.

## The canonical verified-read recipe (existing API only)

Order matters; each step is a real pub fn.

```
// 1. error-frame rejection FIRST (frames pass integrity, so this must precede it)
if fgtw::client::is_error(&body, "not_found") { return Ok(None); }          // expected absence
if let Some((reason, detail)) = fgtw::client::error_frame(&body) {
    return Err(format!("fgtw {op} {reason}: {detail}"));                     // never fall through
}
// 2. integrity + version (whole doc): BLAKE3(doc, ge/gp/gr zeroed) == header hp; enforces version window
vsf::verification::is_original(&body).map_err(...)?;
//    for device-signed frames also: verify_file_signature(&body)? + extract_signer_pubkey(&body)? → check fleet membership
// 3. locate section: VsfHeader::decode(&body) → find TOC field by name → offset_bytes  (or header_end for single-section)
// 4. schema parse (structural + TypeConstraint only — NO crypto, NO required-field check): SectionBuilder::parse(schema, &body[offset..])
// 5. typed extraction: sec.get_value::<T>("field")   (FIRST value only; get_fields for repeated)
```

`SectionBuilder::parse` verifies **nothing cryptographic** — it accepts any structurally-valid forgery. `is_original` + `error_frame` are mandatory separate preceding calls.

## Bug → root cause (confirmed vs contested)

- **Bug 2 (avatar blank / upload broken) — FULLY ROOT-CAUSED, high confidence.** Three FGTW avatar paths (`download_avatar` avatar.rs:1519, `download_avatar_from_seed` :1574, `sync_avatar_bidirectional_from_seed` :1663) gate on `status()` only and **cache the body BEFORE validating**, so a 128-byte error frame at HTTP 200 is written into the vault as an avatar. The sync path's error frame even carries a fresh `creation_time` that WINS newest-wins and can clobber a good local avatar. `upload_avatar_from_seed` treats a rejection frame as upload success, so the real avatar never publishes. Poisoned caches never self-heal (cached frame fails `parse_compressed_image` silently → `None` → refetch → re-poison). The ONE correct path (P2P `AvatarReceived`, photon_app.rs:9016) validates-before-caches and is the model to copy.

- **Bug 1 (handle taken after wipe) — TWO AUDITORS DIVERGE, needs one log line to settle.**
  - *Attest auditor:* `handle_query.rs:565-569/743-746` infers "taken" from the **unverified announce peer echo** — fires `AlreadyAttested` on any non-empty peer list not echoing our record. Claims this is a false positive (reaching it required `ensure_member` to already prove membership), and its consumer (photon_app.rs:5175) clears the session on that false evidence. Fix: delete the peer-echo inference; "taken" only from a fold-verified chain naming a different identity.
  - *Codec auditor:* BOTH taken-sites are ALREADY error-frame-aware, so bug 1 is **stale server state / Cloudflare KV read-lag** serving the pre-wipe chain (matches the keyring OPS TRAP: deterministic device keys + a surviving chain). Fix: verify the wipe covers the `fleet_get` keyspace + peer table; dev-log the raw fetch body on Taken.
  - *Resolution:* not mutually exclusive — the peer-echo inference IS a real false-positive bug worth deleting, AND KV lag could feed it. **Action: do both — delete the unverified inference (correctness regardless) + dev-log the raw body on Taken (settles KV-lag).** The session-clearing rework is the one destructive change → confirm before shipping.

- **Bug 3 (one-way presence: a peer can't see peer-B) — STRONG CANDIDATES, not pinned to one.**
  - A: `bootstrap.rs:524` `parse_peer_from_field(field)?` — **one bad record aborts the WHOLE peer list** → a peer gets zero peers → never dials peer-B. Fix: per-record skip + loud log.
  - B: 2048-byte UDP RX buffer (status.rs:1088) truncates sync-record-laden pongs → parse error, asymmetric by conversation count. Fix: raise to 65536.
  - C: `add_peer` (unverified, admits) vs `merge_peer` (verify-gated, drops unsigned gossip) admission asymmetry — who sees whom depends on announce ordering.
  - All three fixes are safe read-side improvements regardless of which is THE cause.

- **Bug 4 (CLUTCH stuck) — downstream of bug 3** (no independent decision site; the ceremony needs the Online peer_addr).

## What "(VSF verified)" actually means today

`verify_file_signature() == Ok(true)`: ed25519 over BLAKE3(file, `ge` zeroed) against the `ke` pubkey **embedded in the same datagram**. Self-attesting — proves integrity + key possession, NOT who the key is. The contact-allowlist trust gate runs AFTER the full ~500KB parse (and is MISSING on the UDP-direct ClutchComplete branch). `is_original` (provenance) is never checked on any network path.

## Fix plan

### Phase A — vsf crate (all read-side-safe, no wire byte-output change; unit-testable)
- `VsfType::as_u128` (u7 is currently unreachable by any accessor); optionally `as_u16`/`as_u32`.
- `VsfType::as_bytes`: add `hP|hR|hI|hV|ke|kx|kc|ka|ge|gp` arms (photon reads handle-proofs as `hP`, blobs as `ge` — as_bytes fails on exactly those today).
- `FromVsfType`/`IntoVsfType` for `u128` (`VsfType::u7`); `FromVsfType` for `[u8;32]` and `[u8;64]`.
- `FromVsfType for Vec<u8>`: add missing hash/key arms; split the lax `v(_,_)` arm so encoded blobs can't masquerade as plain bytes.
- `FromVsfType`/`IntoVsfType` for `EtType` (every reader hand-pins `EtType::e6`).
- `official::error_schema()`; the fgtw frame schemas.
- `SectionBuilder::parse_document(schema, doc)` — the missing whole-doc entry point (decode header → locate section → parse) that composes the recipe so callers stop skipping steps.
- **`SectionBuilder::parse` fix:** it currently HARD-FAILS on an unknown named field (contradicts its own doc) — a cross-build wire hazard while peer-B/a peer/a peer run mixed builds. Store unknown named fields un-validated instead. (Read-side; no wire change.)
- Do NOT change the existing `Vec<u8> → hb` writer impl mid-test (add a `Bytes` newtype instead if an honest bytes type is wanted).

### Phase B — photon read-side fixes (all zero wire byte-output change)
1. **Avatar (bug 2):** all 4 sites — `error_frame`/`is_error` branch first; move `save_avatar_to_cache*` AFTER `is_original`+decode succeeds; on `load_cached_avatar*` decode failure, `delete_addr` the vault entry (self-heals poisoned test vaults, no nuke). Also validate vault bytes before P2P-serving them (don't sign+ship a poisoned frame to a friend).
2. **Presence (bug 3):** `bootstrap.rs` per-record skip (don't abort the list); raise the UDP RX buffer 2048→65536.
3. **Attest (bug 1):** delete the peer-echo "taken" inference — announce success ⇒ ours; "taken" only from a fold-verified chain naming a different identity; indeterminate (fold/parse/transport err) → keep session, log, retry (never taken, never clear session). Dev-log the raw fetch body on Taken to catch KV lag. **[destructive — touches session-clearing — confirm before shipping]**
4. **CLUTCH ingest:** hoist the contact gate BEFORE the ~500KB section parse; add the `is_known_sender` gate to the UDP-direct ClutchComplete branch (parity with TCP/PT); turn silent-default KEM parses into hard errors; guard the dev `[..8]` slice panics.
5. Widen exact-width extractor matches (`e6`-only, `t_u3`-only, `u4`-only port) via the new `from_vsf_type` helpers.

### Phase C — discipline
- A `dev.sh`/CI gate that fails on NEW `.flatten()` / hand-matched `VsfType::` parses in network-facing files (the "make the next violation scream" step — rules unenforced by tooling don't survive).
- This doc's debt register (below) tracked, not force-converted.

## Debt register — RESOLVED 2026-07-06 (photon 71d0be9)

The register turned out to be mostly corpses: CLUTCH persistence had become memory-only no-ops, orphaning its whole serialization layer.
- `contacts.rs`: reader → `SectionBuilder::parse(contact_state_schema)`; 331 dead lines (slot/secrets/KEM parsers) deleted
- `friendship.rs`: reader → `SectionBuilder::parse(chains_schema)`
- `types/message.rs`: DELETED (zero callers; `ChatMessage` in contact.rs is the real type)
- `crypto/clutch.rs`: all six VSF fns deleted (zero callers); the file contains no VSF parsing now
Phase C shipped too: `scripts/lib/vsf-gate.sh` — a per-file baseline ratchet on raw `VsfHeader::decode`/`VsfSection::parse` in network-facing files, wired into every build. New hand-rolls fail the build.

## Staged (writer byte-output changes → wire-compat risk while mid-test — deferred)

Schema-API rewrite of `FgtwMessage::to_vsf_bytes` + the CLUTCH builders; signing LAN beacons; authenticating pt_ack/nak/ctrl/done; capping/paging pong sync_records; self-signed peer records from the worker; header-signed announce responses (would let clients gate presence on the FGTW key — the real fix for bug 3's trust gap, but needs worker + rollout).
