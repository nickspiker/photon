# Ideas & fixes — anonymous feedback from the field

**Status:** BUILT 2026-10-03; public list + ticker 2026-10-28 (Nick: "an ideas/fixes page for people to submit anonymous feedback about Photon for improving. Since each report would have an ID, provenance hash or whatever's clever, they should be able to see their submitted and status on each too").

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

- `gripe_put {id, nonce, kind, text}` → `gripe_put_ack {id}`. Idempotent: the same gripe twice is one gripe. Text ≤ 4 KiB, UTF-8. A new gripe broadcasts the hub event `idea`.
- `gripe_get {id}` → `gripe_get_ack {state, note, version}`. Anyone holding an id may ask; the id tells nobody who sent it.
- `gripe_list {}` → PUBLIC (Nick 2026-10-28: "Always public, it's free software for everyone, I don't hide anything unless it lets someone steal from me"). A `gripe_list_ack {count}` head section, then one `idea` section per gripe with typed fields: `id` (hb), `kind` (a), `state` (a), `received` (e6), `text` (v t), `note` (v t), `version` (a). The old tab-joined `rows` form is gone, and so is its reader.
- `gripe_status {token, id, state, note, version}` → `gripe_status_ack`: the developer's, gated on the `GRIPES_TOKEN` worker secret. The token lives in keys/gripes.token, never in a repo. A status set broadcasts the hub event `idea` too.

States: received, seen, planned, working, fixed (with the version), declined, duplicate.
The page shows them in plain words: Received, Seen, Planned, In the works, Done (in the version), Not planned, Already asked.

## The page

Settings → Ideas: the explainer, a box, "Send as an idea" / "Send as a fix", then:

- While words are in the box: "Already in, sharing your words" — every public gripe that shares a word of three or more letters with the box, the most shared first. This is the "is it already in the bucket" check, before sending.
- "Tell me when someone sends an idea or a fix": the ticker switch, `ideas.ticker`, device-local, OFF by default.
- "Yours": everything this device sent, newest first, as `<id prefix> · kind · state [version]`, its text, and the developer's note when there is one.
- With the box empty: "Everyone's" — the whole public list, newest first.

Opening the page reloads the sent list, asks the worker for each one's status, and fetches the public list; a send adds its row at once as "sending", then "received" on the ack, or "not sent" with the reason in a toast.
The box is a single line for now.

## The ticker

Every client hears the hub's `idea` event; only a device with the ticker on (or with the Ideas page open) fetches the list on it.
The first fetch of a session is the baseline; after that, every gripe whose id is new since the last fetch, and not one this device sent, raises a notification titled "New idea" or "New fix" with the gripe's text.
Not built yet: an FCM topic for the ticker, so a phone in deep Doze hears it too; today the phone hears it while Photon's service holds the hub connection.

## The developer's side

```
photonlog --gripes
photonlog --gripe-status <id 64hex> <received|seen|planned|working|fixed|declined|duplicate> [note] [version]
```

`--gripes` needs no token (the list is public) and prints one row per gripe, oldest first: id, kind, state, received (UTC, display only), text, then the note and version when set.
`--gripe-status` takes the token; a status set is what everyone sees on their next look at the page.
