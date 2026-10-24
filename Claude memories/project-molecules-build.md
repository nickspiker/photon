---
name: project-molecules-build
description: "MOLECULES (never 'groups'): phase 1 BUILT 2026-09-15; join-by-replay SCOPED 2026-10-01 (docs/molecules.md §11); open = live strand pull, never-friended receive, sponsor re-offer, Withdraw pill"
metadata:
  type: project
---

Canonical: docs/molecules.md.
**Naming (Nick 2026-09-15, "Groups is not it"):** a conversation you found alone is an ATOM; binding a second person makes a MOLECULE; friendships stay distinct. Code, storage domains, wire prefix `photon-molecule`, hash domains `PHOTON_MOLECULE_*`, settings and all catalogs say molecule/atom/bond — never "group" in new code or prose.
Rulings: "a friendship with more people", no owner, no registry name; consent goes with the device owner; secrets only move as WRAPS to a published per-device KEM bundle (match the bundle id BEFORE decapsulation — the HQC decapsulator aborts on a foreign key); revocation = vouch and withdraw (every vouch record kept).
Built (phase 1): roster v2, molecule rows in the Ready list, `Contact::molecule_view` (never in `contacts`), the offer card + Bind, fan-out send (`molecule_transmit`, per-member ACK ledger), token-scoped RX admission (`molecule_admits`), the roster riding the chains blob, MoleculePanel pages, Leave, eras (mint on lost standing, minter cutover on the first wrap ACK).
**Join-by-replay (scoped 2026-10-01):** a from-genesis joiner gets NO rows today. Order: (a) rows carry lane label, author device, plaintext_hash, msg_hp, weave refs, era, strands as raw bytes; (b) a molecule-token history request served from genesis iff the founder flag; (c) the joiner replays each lane; (d) only then the weave.
Open: live strand pull (molecules send ANCHOR-ONLY until it lands); the never-friended receive commit (needs a peer-view resolution, not a Contact row); sponsor re-offer from MoleculeLocal.offered; the Withdraw pill; a fractional delivered ring.

**Why:** Nick's ruling chain of 2026-09-15.
**How to apply:** never flip `anchor_only` before (a)–(c) and the strand pull; never re-serve a MOLECULE_PREFIX row bare (its payload rode the package only).

**2026-10-24 field (strawberry, Nick → Emma):** (1) a roster edge never replicated — chain-sync pushes only advanced lanes, a fresh atom has none → fixed: lane-less frame when the roster digest changed (molecule_roster_pushed). (2) the joiner's group was invisible until the wrap → now listed as Joining at the Join tap. (3) The join answer (wrap) needs a standing device of the sponsor's fleet holding the root — with replication fixed any sibling can answer. OPEN, proposed not built: Add page as a contacts-style tap-to-offer picker, the offer as a distinct card (Join / Not now, state in place), and an "invited" dashed row on the contacts list.
**2026-10-24 invitations BUILT (33cd2638, dev-installed on the desktop only):** Nick's model — decline = UNDO ("like I never sent it"): invitee Not now (offer strip slot 16) forgets the offer + sends MoleculeSignal::Decline (sub 5, gid in gpl); sponsor forgets `offered`, tombstones its offer row and sends a Delete marker → every invitee device tombstones the bond-offer row and forgets the offer. Ready filter Invitations (slot 5, shown only with open offers, count). Sponsor row line "invited: … · waiting". Still open: Add page as a contacts-style tap picker; the offer as a distinct card; "Bind" copy vs Nick's "accept/join"; offers keyed by gid (one sponsor's several pending → lookup by sponsor alone can pick wrong).
