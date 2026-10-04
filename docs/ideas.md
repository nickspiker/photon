# Ideas & fixes — anonymous feedback from the field

**Status:** BUILT 2026-10-03 (Nick: "an ideas/fixes page for people to submit anonymous feedback about Photon for improving. Since each report would have an ID, provenance hash or whatever's clever, they should be able to see their submitted and status on each too").

## The shape

A gripe is a short text and a kind, idea or fix, sent from the Settings → Ideas page.
It carries NO identity: no handle, no device key, no signature.
Its id is blake3(text ‖ nonce) with a nonce the sending device minted and keeps; the worker re-derives the id from the text and nonce it receives and refuses a mismatch, so an id is provenance of its own text and nothing else.
The worker sees the sender's address, as any HTTPS server does, and keeps nothing of it beyond its activity line.

## Where it lives

- `photon-gripes/<id>.vsf` on FGTW's R2: the gripe (kind, text, received time).
- `photon-gripes/<id>.status`: written by the developer — state, note, version. Absent = received.
- `gripes.mine` on the sending device, device-local: the ids it sent, each with its kind, text and time (a VSF document of multi-value fields).

## The routes (fgtw-bootstrap worker)

- `gripe_put {id, nonce, kind, text}` → `gripe_put_ack {id}`. Idempotent: the same gripe twice is one gripe. Text ≤ 4 KiB, UTF-8.
- `gripe_get {id}` → `gripe_get_ack {state, note, version}`. Anyone holding an id may ask; the id tells nobody who sent it.
- `gripe_list {token}` → `gripe_list_ack {rows…}` and `gripe_status {token, id, state, note, version}` → `gripe_status_ack`: the developer's, gated on the `GRIPES_TOKEN` worker secret. The token lives in keys/gripes.token, never in a repo.

States: received, seen, planned, fixed (with the version), declined, duplicate.

## The page

Settings → Ideas: the explainer, a box, "Send as an idea" / "Send as a fix", then "Yours": everything this device sent, newest first, as `<id prefix> · kind · state [version]`, its text, and the developer's note when there is one.
Opening the page reloads the sent list and asks the worker for each one's status; a send adds its row at once as "sending", then "received" on the ack, or "not sent" with the reason in a toast.
The box is a single line for now.

## The developer's side

```
photonlog --gripes
photonlog --gripe-status <id 64hex> <received|seen|planned|fixed|declined|duplicate> [note] [version]
```

`--gripes` prints one row per gripe: id, kind, state, received (unix seconds), text.
A status set is what the sender sees on their next visit to the page.
