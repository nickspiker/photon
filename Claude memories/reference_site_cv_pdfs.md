---
name: reference_site_cv_pdfs
description: "holdmyoscilloscope.com site lives at /mnt/Chiton/MEGA/holdmyoscilloscope (deploy.sh = wrangler pages); CV PDFs regenerate via about/make-cv-pdfs.sh (headless google-chrome, A4, pages carry their own @page print CSS)"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 0b164fd9-062c-407e-b4bb-8f6be8d1982d
---

The personal site holdmyoscilloscope.com is a Cloudflare Pages project at `/mnt/Chiton/MEGA/holdmyoscilloscope` (its own git repo; `deploy.sh` = `wrangler pages deploy`).

The three CV PDFs (`about/Nick Spiker - {Systems,Network,Field} CV.pdf`) are generated FROM the HTML (`about/cv-{systems,network,field-rf}.html`) by `about/make-cv-pdfs.sh` — headless google-chrome `--print-to-pdf`, no header/footer; the pages carry their own `@page` print CSS.
Re-run the script after ANY cv-*.html edit, or the PDFs silently drift from the pages (that drift already happened once — created 2026-07-03 after the "probably secure" de-arrogance pass).

Copy doctrine (from that pass): probability where it's probability, proof where it's proof — "probably secure messaging" is the photon GitHub tagline; absolute claims ("impossible", "provably", "first ever") only survive when scoped to a threat model or backed by a deterministic mechanism.
