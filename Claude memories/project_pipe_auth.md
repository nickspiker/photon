---
name: project_pipe_auth
description: fgtw.org relay pipes prove the device key (2026-11-18); window for unsigned clients OPEN until the owner closes it; checklist before closing
metadata:
  node_type: memory
  type: project
  originSessionId: 17b90e8e-8b1a-448e-8ac7-22b057e9a0d7
  modified: 2026-11-18T23:59:00.313Z
---

The hole: `/pipe?dev=<id>` opened for any id with no proof, so anyone could take over or knock off another device's relay pipe (found 2026-11-18 by the ferros-TCP work).
Built 2026-11-18: fgtw d9dee2d (`fgtw::pipe` handshake), fgtw-bootstrap 1507e35 (`src/pipe_gate.rs`: challenge, proven pipes only, unproven never displaces proven), Photon branch `pipe-auth` 3ecb6b34 (`relay::pipe_answer`, not merged).
fgtw-bootstrap f1434ed sets `UNSIGNED_PIPE_WINDOW = true` (`MIGRATION-EXPIRES: 2027-02-01`) so a deploy never cuts released clients; deploy with the window open FAILED 2026-11-18 (/home full: wrangler ENOSPC, then its login refresh); fgtw.org still c688671a; redeploy once /home has space.
**Why:** the owner's rule is that a replaced path dies the same day, but no released Photon answers the challenge yet; closing at once cuts every released client off the relay.
**How to apply, before closing the window (owner's call; ferros recommends release first, then a short date):**
1. Merge `pipe-auth` into main (DONE 2026-11-18, photon 77b96ab2) and ship it in a release; people update.
2. The rustdesk fork's `svc=rd` pipe answers with `fgtw::pipe::open_answer`.
3. Photon on ferros (branch `ferros-tcp`): in the ws pump callback in status.rs (~2028) ignore empty frames, send `relay::pipe_answer(&data, &keypair)` with `ws.send_binary` and return true, pass a keypair clone into the thread.
4. Flip `UNSIGNED_PIPE_WINDOW` to false (or delete the window) and redeploy.
Related: [[project_ferros_tcp]] (Photon's ferros TCP, branch ferros-tcp), and fgtw.org's unsigned error replies, which must be signed before ferros uses plain HTTP.
