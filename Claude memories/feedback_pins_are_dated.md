---
name: feedback-pins-are-dated
description: "HARD (Nick 2026-11-03): a compatibility pin's deletion is a calendar date ('delete not before YYYY-MM-DD'), never a version count; three months is the floor; the three live pins (Android root, provenance, old-self import) all end 2027-02-01"
metadata:
  type: feedback
---

A pin (a legacy reader, a conversion path, a read-both branch) is marked for deletion by a calendar date, never by a version number: `MIGRATION-EXPIRES: 2027-02-01 — why`, the marker scripts/lib/migration-gate.sh scans, never `delete at v168`.

**Why:** Nick pushes about a version a day, and a few of his machines go a month without updating. "Thirty versions" is a month of wall time, so a laggard jumps the pin and is stranded: a phone that misses the Android root conversion keeps a vault no code can read; a client past the provenance pin cannot read anything written before 2026-11-03. The pin's horizon must be sized to the laggard tail, not the release count. (Nick: "if I don't update my machine in a month (which a few haven't) and I push a version a day...")

**How to apply:** every new pin carries `MIGRATION-EXPIRES: <date> — why` on a comment line of its own (the gate greps .rs and .kt in photon and every sibling crate beside it; `MIGRATION_GATE_TODAY=2027-02-01` dry-runs sweep day), with the date at least three months out; the three pins alive on 2026-11-03 (ANDROID ROOT CONVERSION PIN, PROVENANCE PIN, OLD-SELF IMPORT) were re-dated to 2027-02-01 so there is one sweep day. A pin is deleted at the first deploy on or after its date, and only once the manifest's update floor is past the pin's version. The gate reads the clock once on the build machine and refuses the build on or after the date; the client never learns a date existed (edges, not timers). See [[project_android_widevine_plate]], [[project_security_audit]], [[project_old_self_import_removal]].
