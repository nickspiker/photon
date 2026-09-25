---
name: feedback_no_compat_text_hatches
description: "Nick 2026-09-24: never keep a backwards-compatible reader for a text/string encoding — every surviving string parser is an attacker's lever; VSF validates (typed values, strict ASCII field names, provenance), hand parsers don't"
metadata:
  node_type: memory
  type: feedback
---

When a text-smuggled encoding is replaced by typed VSF fields, the old reader is deleted the same day — no dual-read window, no legacy arm, no "transition era". Legacy rows are dropped (and purged), never parsed.

**Why:** Nick: "that gives attackers a hatch, a lever if you will to pull on. we don't want that. VSF validates, makes sure field keys follow specific ascii convention, that sort of thing." A hand parser for an old grammar is a second, unvalidated input path: a crafted message matching the old text makes the code act where the typed path would refuse (the bridge's untyped `$ ` command arm is exactly this — remote execution keyed on text, audit item H4).

**How to apply:** on any flag day that retires a string encoding, ship with NO compatibility read path — drop-and-purge the legacy form at every ingress (live frame, page merge, vault load), and write the reasoning into AGENT.md's "Text Carries Only Human Words" section (done 2026-09-24). Applies equally to transition arms found in audits: delete them, don't extend them. Related: [[project_wave_canonical_container]].
