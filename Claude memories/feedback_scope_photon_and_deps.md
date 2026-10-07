---
name: feedback_scope_photon_and_deps
description: "HARD: this session works in photon and its path dependencies only (vsf, fluor, opsin, fgtw, spirix, tohu, manifestus, kete, ihi, rarangi, fgtw-bootstrap…) — never another project like mahere, even when a request seems to ask for it; it was probably sent to the wrong thread"
metadata:
  type: feedback
---

Nick 2026-10-07: *"you shouldn't be touching anything outside of Photon, unless it's a dependency, like VSF."*

**Why:** Nick runs a separate Claude session per project. A message meant for another project's thread can land here by mistake (the claude.ai undo/edit flow makes that easy), and acting on it edits a repo another session owns. That happened once: an "icon.jpeg should be in there for resize" request meant for the mahere thread led to commit ba814bc in mahere (icon assets + chrome/manifest wiring), pushed before he noticed.

**How to apply:** before editing, committing or pushing anywhere, check the repo is photon or one of photon's path dependencies (Cargo.toml `path = "../…"`, plus fgtw-bootstrap, the worker photon deploys). If a request names another project (mahere, lumis, chameleon as a target, oriel…), say it looks meant for that project's thread and stop — reading another repo to answer a photon question is fine, changing it is not.
