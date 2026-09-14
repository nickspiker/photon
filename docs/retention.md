# Retention — the winnow, the custody ladder, and the sync law

**Status:** DESIGNED 2026-09-14 (Nick + Claude, the vault-stats conversation). Built so far: the custody verbs' first rungs (star/keep, loft, discard, the delete/edit sync law, the Vault page breakdown). The WINNOW ITSELF — budget enforcement and automatic eviction — WAITS FOR THE DEVICE-SYNC PHASE: eviction is only safe once "another device of mine provably holds this" is a checkable fact (see docs/durability.md, project_vault_roadmap).

## 1. Why "winnow", not garbage collection

GC reclaims the unreferenced. Retention discards things that are still perfectly referenced, on purpose, by policy — after a size or an age, whichever bites first. The engine already speaks farming (plow, reap), so: **winnow** — keep the grain, let the wind take the chaff.

## 2. The budgets (Nick's shape)

Per-device allocation, then per-category envelopes, each with a size cap AND an age window — both are ceilings, content must fit under both:

> "128 GB on my phone, 50 I want to allocate to Photon, 5 to messages themselves or 1 year, whichever trims more, then photos 1 month or 20 GB…"

Enforcement on EDGES, never a sweeper: a write that would breach the envelope winnows first, plus one pass at launch. Budgets live as typed settings (device-local — the phone's budget is not the desktop's).

## 3. The custody ladder

A strict ladder; the winnow moves things DOWN one rung within its budgets, the user moves things either way, keeps never move unless unkept.

| rung | verb / state | where the bytes are | wired today |
|---|---|---|---|
| kept | **keep** (★, `star_osc`) | everywhere, forever — winnow-exempt, replicates with priority | ★ pill on every row; the Kept bin on the Vault page |
| passing | (default) | here for now, fleet forever | — |
| lofted | **loft** (en loft / es al palomar / mi whata) | fleet only: local bytes dropped, row + preview + re-fetch stay | v1: incoming pigeons only (sender's fleet holds the original); slot 12 |
| discarded | **discard** | nowhere of yours: fleet-wide tombstone, space at compaction | shipped long since |

Graduated eviction beats deletion: full res → big preview → thumbnail → bare row; the timeline never loses its face ("the row IS its waveform" holds at a few KB forever). Loft v1 excludes call recordings and our own uploads — no custody proof yet; the `replicated` flag (row-level) is NOT blob custody.

## 4. The sync law: authorship propagates, experience doesn't

- A **chirp is your utterance** — author-signed records about it travel and every participant's view updates: **edit** (a `RefKind::Edit` row superseding by reference) and **delete** (the hidden delete marker → tombstone). Enforced 2026-09-14 on both ends: the marker rides only for rows WE authored; a received marker tombstones only rows the SENDER authored (incoming here), never ours, never a wave; an edit supersedes only rows travelling its own direction.
- A **wave is a shared event** — each side's archive is their own memory of it (the wire copy is the good copy of every party). Discard never crosses the wire for a wave, in either direction.
- Retraction is **cooperative**, not magic: photon's clients honor it; the participant already saw the message. Documented promise, not a claimed recall.
- The one collision, decided: a recipient's explicit **keep made before the author's retraction stands** (their vault, their deliberate act — the same reason keeps outrank the winnow). Not yet enforced in code (keeps and retractions don't consult each other today); wire it when the winnow lands.

## 5. Edit history

Rows persist regardless — braid weave makes row content key material, so "remember history" is honestly a VIEW toggle, never storage. `chat.history` (linked, default OFF): a selected edited bubble's meta lists every prior version with its age. True content shredding needs the braid-safe redaction design (ticketed, see ChatMessage::deleted).

## 6. The Vault page breakdown

Per-conversation totals, largest first, filters everything / waves / pictures / songs / files / ★ kept — a pure in-memory walk over row records (every attachment row carries its size), no disk, recomputed on page entry / Refresh / filter taps. The engine's occupied number stays ground truth for the whole vault; the walk is what the rows claim. A later `VaultOp::Sizes` enumeration (kete's index already holds every length in memory) can add held-here-vs-lofted accounting per key when the winnow needs it.
