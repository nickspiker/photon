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

Graduated eviction beats deletion: full res → big preview → thumbnail → bare row; the timeline never loses its face ("the row IS its waveform" holds at a few KB forever). Loft v1 excludes wave recordings and our own uploads — no custody proof yet; the `replicated` flag (row-level) is NOT blob custody.

## 4. The sync law: authorship propagates, experience doesn't

- A **chirp is your utterance** — author-signed records about it travel and every participant's view updates: **edit** (a `RefKind::Edit` row superseding by reference) and **delete** (the hidden delete marker → tombstone). Enforced 2026-09-14 on both ends: the marker rides only for rows WE authored; a received marker tombstones only rows the SENDER authored (incoming here), never ours, never a wave; an edit supersedes only rows travelling its own direction.
- A **wave is a shared event** — each side's archive is their own memory of it (the wire copy is the good copy of every party). Discard never crosses the wire for a wave, in either direction.
- Retraction is **cooperative**, not magic: photon's clients honor it; the participant already saw the message. Documented promise, not a claimed recall.
- The one collision, decided: a recipient's explicit **keep made before the author's retraction stands** (their vault, their deliberate act — the same reason keeps outrank the winnow). Not yet enforced in code (keeps and retractions don't consult each other today); wire it when the winnow lands.

## 5. Edit history

Rows persist regardless — braid weave makes row content key material, so "remember history" is honestly a VIEW toggle, never storage. `chat.history` (linked, default OFF): a selected edited bubble's meta lists every prior version with its age. True content shredding needs the braid-safe redaction design (ticketed, see ChatMessage::deleted).

## 6. The Vault page breakdown

Per-conversation totals, largest first, filters everything / waves / pictures / songs / files / ★ kept — a pure in-memory walk over row records (every attachment row carries its size), no disk, recomputed on page entry / Refresh / filter taps. The engine's occupied number stays ground truth for the whole vault; the walk is what the rows claim. A later `VaultOp::Sizes` enumeration (kete's index already holds every length in memory) can add held-here-vs-lofted accounting per key when the winnow needs it.

## Eviction keys on RECEIPT, not creation (Nick 2026-10-07)

*"On the timeline, we need a way to mark when we received the blob, wave, pigeon, whatever and be able to evict based on that rather than creation time."*

- **What is hashed.** A file pointer is BLAKE3 over the raw bytes (no VSF wrapper, no timestamp inside), so the same file sent at any time from any device is the same hash — re-sending never stores a second copy. Only an attachment's HEAD is a timestamped VSF document, and the head is not identity.
- **The receipt record.** Each device keeps a LOCAL record per content hash: `first_here` (the bytes landed here — push, fetch, replication or our own send) and `last_wanted` (the last time a photon arriving here pointed at it). A re-send of a held file refreshes `last_wanted` on every device that receives the new photon, moving no bytes. Never on the chain, never synced: it is this device's experience, not the conversation's history.
- **Eviction** (the category horizons above) compares `last_wanted` against the device's window, drops the BYTES only, and leaves the photon's pointer; the file re-fetches by hash from any holder. Re-sending a file is how a person keeps it alive everywhere.
- **The timeline** shows "here since …" beside "on this device" in the file's details.


## The limits, built 2026-11-17 (keep.rs)

Nick 2026-11-17, superseding the 2026-10-21 "free-space floor only" call: *"would be nice to have the sliders for audio, video, pigeons and photons adjustable... I really don't need to keep record of every single frame of a beam I made from 6 months ago."*

- **Limits.** Waves (their sound), beams (their video) and pigeons (files and pictures) each have an age limit and a total-size limit; whichever trims more wins. The Vault page draws six sliders, continuous in doublings (age: a day to about eleven years; size: 64 MiB to 1 TiB), the right end of each being no limit. Keys `keep.<kind>.age` (oscillations) and `keep.<kind>.size` (bytes), absent or `u0(false)` = no limit. A slider writes on release.
- **Whose limits.** This device's own, or the fleet's when it follows the fleet (the Beam page's "This device only" / "Follow my fleet" switch over the same per-key link). Either way the device trims to them without asking a sibling for a second copy: following the fleet means the fleet's limit is the limit everywhere that follows it, and a device kept independent (a desktop archive set to keep everything) is the owner's call.
- **What goes.** Only the bytes, oldest first, never a starred (★ kept) row, never a wave's envelope. The photon, its preview and its waveform stay. Blob bytes never enter key material (the braid weaves a row's pointer), so a trim leaves every braid intact. A trimmed hash joins `space.evicted`: automatic fetches skip it, and they skip anything past this device's age limit; a deliberate Play or Save fetches it again from any holder.
- **Edges.** Session up, a limit changed (here or arriving from the fleet), a recording kept, any bytes landing. The choice runs on a worker that probes what is held here, so sizes count only what this device actually holds.
- **Ages** count from the row's time; the receipt record above is still unbuilt.

**Photons are not trimmed yet.** The weave reaches back into rows: a sender weaves two of the peer's newest 256 non-control rows, and the receiver resolves each woven time across all its rows, holding the frame and walking history if one is missing. So a device must keep at least the newest rows each peer may still weave against, which while a peer is offline can be more than 256. The larger obstacle is convergence: the anti-entropy digest and the settled root hash every non-deleted row, so a row pruned on one device is re-served to it by its siblings and the history sweep. Trimming photons needs a per-conversation "pruned below T" floor that the digest, the merge, the backfill server and the sibling sync all respect, with a floor of kept rows above the weave window. Designed when built.
