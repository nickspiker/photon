---
name: feedback_fgtw_out_of_photon
description: HARD: everything FGTW (fleet, identity, attest, phonebook, device add/join/depart flows, their wording) lives in the fgtw crate; Photon is becoming just a messenger
metadata:
  type: feedback
---
Anything FGTW-related moves OUT of Photon into the fgtw crate (Nick 2026-11-10, after saying it "three times now"). That covers the flows, not only the primitives: add / join / depart sequences, select verification, chain and key pushes, the fleet page's behaviour, and the wording (fgtw text.rs).

**Why:** ferros has no Photon fleet page; there Photon is just a messenger and the OS owns identity and fleet. On Linux, macOS, Windows and Android, Photon only *hosts* FGTW/TOKEN management, so ferros must inherit the behaviour from fgtw, not re-implement it. On 2026-11-10 the Yes/No green step and the depart_req resend were each built twice (Photon and ferros) because the flow lived in Photon.

**How to apply:** new fleet/identity behaviour goes in fgtw first (UI-free state machines that return actions; each app supplies radios, drawing, vault), and Photon calls it. Never add fleet logic to photon/src/ui/photon_app/ even as a "quick fix". If a change seems to need it, put it in fgtw or ask. docs/one-identity.md "Photon keeps ... fleet" is superseded by this. Related: [[project_one_identity]], [[feedback_any_size_conversation]].
