# Durability, the Horizon, and Crypto-Shred — the §14.7–14.8 arc, re-derived over lanes

**Status:** DECIDED 2026-08-18 (design; not yet built). This document closes the last open fork of the fleet cutover: braid.md §14.5 specced an always-online, fleet-key-sealed **message slot** on FGTW that was never built, and §14.7–14.8 (the CAN-resync guarantee, the prune horizon, crypto-shredding) were written assuming it. Meanwhile the tree grew a different answer: per-device lanes (docs/lanes.md), fleet history replication (rarangi rows + history pages sealed under spine epoch keys), and the epoch spine with custody. This doc re-derives the durability arc from what exists. Companion to `braid.md` §14; supersedes §14.5/§14.7/§14.8 where they disagree, in the lanes.md tradition.

## 0. The fork, and the decision

**Option A — build §14.5 as written:** a grow-only, content-addressed, fleet-key-sealed set per friendship in R2. Buys: wiped-*sole*-device history recovery, and a resync guarantee independent of sibling liveness. Costs: message content (sealed) parked on rented infrastructure forever — a retention/subpoena/traffic-record surface that exists only because we created it; a second sync protocol (have-set digests, union merge) beside the lane/history machinery that already converges in the field; horizon + cursor + shred logic operating against a remote store; standing R2 cost and abuse surface; months of work in front of voice calls.

**Option B — fleet-holds-history (DECIDED):** the fleet IS the durable store. Message content never touches infrastructure, sealed or not — the always-online footprint stays exactly what it is today: the roster/key fan-out slot, checkpoint custody, and blind deposits (small frames, store-and-forward toward a handle proof). Durability = replication across your own devices; the resync guarantee is re-stated over sibling reachability (§2). Horizon and crypto-shred become **local** operations (§3–§4), which makes them *stronger*, not weaker — there is no third-party copy to fail to destroy.

Why B: it matches the privacy posture the rest of the system bleeds (handles are secrets; nothing pubkey-derived on the wire; the worker learns as little as we can arrange), it's the architecture the last six weeks of field soak actually hardened, and its one real loss — the wiped-sole-device case — is honestly a **backup** problem, not a **sync** problem, and is served by a cheaper, user-sovereign mechanism (§5) than a standing cloud set. Reversal path if this proves wrong: the §14.5 slot layers ON later without unwinding anything here (the history-page seal keys and checkpoint bounds this doc introduces are exactly what a slot would be sealed under).

## 1. What statelessness now means (amends §14.0)

A device still keeps one durable unique secret: its `ihi`. But "everything else is re-fetchable" is re-scoped:

- **Re-fetchable from infrastructure:** fleet key (fan-out slot, `ihi`-unwrappable), roster fstate, checkpoint custody, doorbell/deposit frames. The *coordination* minimum.
- **Re-fetchable from the fleet:** message history, conversation state, friendship chains (lane checkpoints ride the replication blob). The *content* plane.
- **Nowhere but the fleet:** exactly the content plane. A fleet reduced to zero live copies has lost its history — by design, the same way a fleet reduced to zero devices has lost its identity's continuity. The vault remains a disposable cache; the FLEET is the source of truth.

UI obligation (new, small): a single-device fleet is a fleet whose history has one copy. The device-list screen states it plainly — "history lives only here; add a device to replicate" — the same honest-surface doctrine as the carries-an-identity prompt.

## 2. The CAN guarantee, re-stated (supersedes §14.7's slot form)

> Any current-member device offline **≤ W** can resync deterministically from **any one live sibling** holding the rows. Offline **> W**, it may find the fleet's horizon has advanced past its cursor: it rejoins live participation immediately (lanes need no history) and backfills only what the fleet still holds — logged, UI-surfaced, never silent.

- **Cursors.** Each sibling already exposes what it holds (history-page sweeps + lane heads). We make it explicit: a per-sibling **sync cursor** (last checkpoint k through which that sibling has confirmed holding all rows), carried on the existing pong/roster edges — no new frame family, one field.
- **The horizon may not advance past `min(cursor)`** across fold members — **except** past wall-clock grace `W` (months; a user-facing setting, never a count), after which advancing past a silent member is a deliberate, logged, UI-surfaced decision, same wording as §14.7: a lost device can't hold forward secrecy hostage.
- **Convergence gate, fleet form:** before any epoch-key zeroize (§4), at least one OTHER live sibling must confirm holding everything at/below the checkpoint being retired — read back over the sibling link, not assumed. A single-device fleet therefore never auto-shreds (nothing to converge against); its horizon is manual-only (§3).

## 3. The horizon (supersedes §14.7's prune mechanics)

The horizon is a per-conversation retention dial, default **keep-forever** (the current behavior — nothing changes until the user asks):

- Advancing the horizon to checkpoint `C_k` prunes rows below it from vault + RAM on every device (each device prunes on observing the horizon record, which rides the fleet plane like any monotone fact — true-wins, idempotent).
- The horizon record is authored on the device where the human turns the dial, gated by §2's cursor rule, and carries `(conversation, k, authored_osc)`.
- History pages a friend requests below our horizon get the honest answer that already exists: `more=false` at the boundary — the friend's own fleet keeps its own copies under its own horizon; we never promise to be their archive.

## 4. Crypto-shred (re-derives §14.8, and it gets stronger)

§14.8's insight — deletion of remote bytes is best-effort, FS = **on-device key destruction** — survives intact and simplifies, because there are no remote bytes:

- History pages are already sealed per-epoch (`fleet_epoch_seal_key(epoch_k, hist_page)`, spine-derived). The shred at horizon `C_k` = zeroize every epoch seal key `< k` from the reservoir lineage on every device, after §2's convergence gate. Any pre-horizon page still in flight, cached in a relay, or recorded off the wire is thereafter unopenable by anyone, including us.
- Vault rows below the horizon are deleted outright; the vault seal (device-key) plus the row deletion is the local story, and we *document* (and mean) that the guarantee is the key destruction, not the disk overwrite — same doctrine, smaller surface.
- The friend-facing plane is untouched: lane chains ratchet forward as ever; this arc governs only the fleet's stored history.

## 5. The wiped-sole-device case — a backup, not a slot

The one thing Option A bought that B doesn't: a single-device user who wipes that device. The answer is an **export**, not infrastructure: a user-initiated, passphrase-sealed history archive (VSF, the vault codec we already have) written to a file/drive of their choosing. Sovereign, offline, zero standing cost, zero retention surface — and it composes with §4 (an export is above-horizon content only). UI: one button next to the retention dial. This is deliberately Phase-later; the honest single-copy warning (§1) ships first.

## 6. Build order (all post-voice-calls unless pulled forward)

1. **Single-copy warning** (§1) — small UI, ships with the next batch.
2. **Sync cursors on the pong edge** (§2) — one field + bookkeeping; useful diagnostics even before any horizon exists.
3. **Retention dial + horizon record + prune** (§3).
4. **Epoch-key shred behind the convergence gate** (§4).
5. **Sealed export** (§5).

Nothing here blocks voice calls; item 1 is the only near-term obligation. The spec debt this doc retires: braid.md §14.5 (slot substrate — explicitly not built, superseded), §14.6 (linearizer — already retired by lanes), §14.7–14.8 (re-derived above).
