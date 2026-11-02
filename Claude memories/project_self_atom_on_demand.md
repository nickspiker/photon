---
name: project_self_atom_on_demand
description: 2026-11-02 FIXED cd741412 — a reset device never founded the self atom (only founded when holding a row) and dropped every sibling page carrying its rows; a sibling page for its token now founds it
metadata:
  type: project
---
Nick 2026-11-02: "my Self atom disappeared" + "Unable to send myself a pigeon" (sent from the desktop into a new "notes" atom he founded on the Pixel instead).
Convicted: adopt_old_self_conversation founds the self atom only when the device's own table holds a non-control row ("an empty one is never founded"); Nick's reset Pixel held none, so it had no atom, and status.rs dropped 80 sibling HISTORY pages for its token ("token resolves to no contact"). The desktop's atom a9a00ab2 (127 rows) was intact.
FIXED: status.rs — a from_sibling page whose token is the derived self atom's (MoleculeId of Conversation::new([us])) sets self_atom_wanted_after; after the pass found_self_atom(Conversation::new([us])) founds it empty (roots derive from the identity seed — identical on every device); the next re-serve lands.

**Why:** a deliberate "never found empty" rule plus a drop gate made a deadlock on any device that lost its local rows.
**How to apply:** verify on the Pixel: look for "a sibling holds rows of the conversation with only us in it" then pages landing. Related: [[project_conversation_convergence]].
