# Group media at any size

Waves and beams in a molecule of any number of people: the screen, the audio, the video and the network, from the eight-device mesh built on 2026-10-25 to a group that has no ceiling.
Nick, 2026-10-31: "do we have a good scaffolding for adding multiple people to the screen so we can do beams and waves with molecules of an unlimited number?" — "Write it all up".

**Status:** DESIGN — nothing here is built. docs/group-waves.md is the group wave as it runs today; this is how it grows. Nick's decisions of 2026-10-31 are folded in (the last section).

## Where it stands (v135)

What already holds at any size:

- **The screen.** The stage lays every member out on one tile grid (`wave_field::face_tiles` over `picture_tiles`): a face with its level ring, or that member's picture. The grid takes any count; tiles only get smaller.
- **Membership and keys.** A molecule's era goes to its members over their bonds in O(N) deliveries (docs/conversations.md), and a member need not be anyone's friend (docs/group-waves.md "A member is a person").
- **Signalling.** Offer, join and leave ride the molecule's lane and go express to every standing device; the roll call goes express to the newcomer alone (v136). None of it caps the member count.
- **Recording.** Each link spools its far party, and the keep merges them into one recording, a channel per party.

What caps it:

1. **Eight slots, fixed** (until v136). `platform::audio::FAR_SLOTS = 8`, and every per-party table was an array of that length; a ninth device logged "no far slot left". Step 1a replaced them with the party table (below).
2. **A mesh of whole engines.** Every pair of joined devices runs a complete 1:1 engine (`derive_group_link_secret`): each device encodes its microphone N−1 times, each with its own ladder, and decodes N−1 streams. CPU on every device grows with N; the group's traffic grows with N².
3. **A beam is uploaded N−1 times.** The camera is encoded once and the fan (`beam_session::GROUP_FAN`) sends every frame to every link: N−1 copies leave each sender's uplink. At five to eight people a home uplink is full.
4. **No path, no media.** A pair needs a direct path either end can reach; a member behind a strict network hears nobody it cannot punch to.

## The goal

Any number of people in one wave or beam, on ordinary connections, with:

- **each sender's uplink independent of N** — one upload per stream, whatever the group's size;
- **each receiver's work bounded** — a fixed number of streams decoded and mixed, however many are present;
- **the same end-to-end security** — media sealed by members for members, nothing in the clear at any relay, FGTW still only a phonebook (docs/peers-are-fgtw.md);
- **the 1:1 wave untouched** — a pair keeps its own engine, its own keys, its own ladder.

Four stages get there. Each is useful on its own and none undoes the one before.

## Stage 1 — parties that come and go; encode once

**Lifts the eight-device cap for voice. Contained to the audio session and the group's link bookkeeping.**

### The party table (built, v136)

Every fixed `[_; FAR_SLOTS]` array becomes one table of PARTIES: a growable list of entries, each holding everything that is per-party today — its capture queue, generation, receive and start times, send and redirect addresses, its picture, its live-field side.
A party is found by its slot number, handed out at claim (`claim_slot` / `release_slot`, with generations); lookups are an index, never a hash map (AGENT.md). One generic `platform::audio::PartyTable` holds every per-party table (audio playout, `wave::SLOT_LINKS`, the beam pictures).
The 1:1 wave is party 0, exactly as slot 0 is now.
The table grows as members join and a released entry is claimed again by the next link; the generation keeps a link that ended and one that reuses its entry from tearing each other down, as it does today.

### Encode once (built, v136)

Today each link's engine reads its own copy of the microphone and runs its own encoder and ladder.
At N links that is N−1 encodes of the same audio.
Instead the group runs ONE send pipeline — capture, the level plan, the encoder — and each link only seals and sends what it produced:

- **One rung for the group.** The encoder runs at the rung the group's paths can carry: the lowest rung any link's verdict asks for (v1 of the beam fan already sizes to one link's verdict — the lead's; this generalizes it). Stage 2 replaces "the worst link" with "the path to the forwarder".
- **Repair stays per link.** The fountain repair copy and the loss loop belong to each path, so each link keeps its own; only the encode is shared.
- **The level plan is one.** The microphone has one makeup and one hot-mic guard, as it already does in the lead link's engine; the other links stop re-running them.
- **Receive is unchanged.** Each party still has its own receiver, its own playout (its own L and cursor) and its own decode.

**As built (v136).** The lead link's engine is the one send pipeline: it runs the level plan and the encoder and publishes each frame — its grid name, the rung, the wire samples and the encoded bytes — into the wave's `engine::GroupTx`; every link, the lead included, reads the frames from its own cursor and seals and sends them, and the other links only keep their copy of the microphone drained for the day the lead passes to them.
Each link's ladder publishes the rung it wants; the encoder runs at the lowest (`GroupTx::rung`, plaid read as the top coded rung) and moves only on eight-frame grid boundaries.
A group link's windows end on the grid — the frame before a multiple of the window's length — and every window length divides eight, so a rung change always falls between every link's windows; a window cut short (a link starting mid-way, or a frame at a new rung after the lead passed) keeps its geometry with empty slots, which the receiver already skips.
A link whose own path is plaid sends the frame's samples bare; a loss on any link drops from the rung it actually sends at (`drop_from`), so the encoder hears it.
A frame name a link has already sent is never sent again, so a new lead whose microphone names frames the old one already published cannot double them.

### What it buys

A device then encodes once and decodes N−1.
Voice at 32 kbit/s per stream: a 16-person wave sends 15 × 32 ≈ 0.5 Mbit/s up — within any broadband uplink.
Beams still upload N−1 copies; Stage 2 fixes that.

## Stage 2 — members forward

**Each stream uploads once. The core of "unlimited".**

### The shape

Each sender sends each of its streams ONCE, to a FORWARDER: a member's device that passes it on to the others.
A forwarder receives every sender's streams and sends each receiver the ones it wants (Stage 3).
For larger groups the forwarders form a tree: a forwarder's receivers may themselves be forwarders for others, the same spanning tree a molecule's era already travels (docs/conversations.md, "Deliver over bonds").
A member with no direct path to another reaches it thru a forwarder both can reach — the relay media 1:1 waves lack, built from members, never from FGTW.

### Choosing forwarders

Every member must agree on who forwards without a round of negotiation, as every device already agrees on a ceremony owner from the roster.

- **Each device states what it can carry** in its join (`Answer`), as typed fields — never text: its measured uplink (the PT path pace's settled window, docs/PT.md), whether it is on mains power, whether its network is metered, and whether it holds a public or port-mapped address.
- **The choice is a pure function of the joined set**: the device with the most uplink that is on mains, unmetered and reachable forwards; ties break by device key. Every member computes the same answer from the same roll.
- **Automatic, on by default, off per device** (Nick 2026-10-31: "automatically choose relays (enabled by default but can be disabled per device)"): Settings → Wave carries "Relay for groups", a device-local key that is never linked to the fleet, because what a machine can carry belongs to that machine. A device switched off states so in its join and is never chosen; it still sends and receives thru whoever is.
- **Mesh while the math allows** (Nick: "whatever the math supports"): there is no fixed group size. Each sender sends straight to every other member while its own uplink carries the copies — `(N − 1) × its stream rate ≤ uplink >> 1`, half the measured uplink, the other half left for everything else on the link (the lesson of the PT path pace, docs/PT.md). Past that, that sender sends once, to the forwarder. So a desktop on fibre may mesh with a dozen people while a phone on cellular forwards at three, in the same wave, each deciding from its own measurement. A direct link costs no extra hop, so the mesh is always preferred while it fits.
- **A forwarder's own budget is the same rule.** Forwarding N streams to N − 1 receivers is N(N − 1) streams up — 50 voices at 32 kbit/s is about 78 Mbit/s — so one forwarder serves only the receivers its uplink carries, and the rest are served by further forwarders below it: a tree, deeper as the group grows, each forwarder's fan-out set by its own measured uplink.
- **A phone forwards only when nothing else can** (on battery, on cellular): it is a sender and a receiver first.
- **Peer to peer, only without a direct path, only when it pays** (Nick 2026-11-01: "Relay peer-peer only, no thru fgtw.org and only if no direct path and relay is higher bandwidth (proper bittorrent ideally)"): a stream is relayed only between two members with no direct path to each other, only thru a member forwarder (never fgtw.org), and only when the forwarder's path carries more than what the pair would get otherwise — swarm-style, each forwarder carrying the share its uplink can. Built so far: group wave express signals go straight to each device's addresses and never thru the fgtw.org relay; a device with no address takes the signal from the group's lane row, which members carry to each other.
- **Losing a forwarder** is a drought on every stream it carried. The members recompute the choice without it; senders re-aim; receivers' playouts ride out the gap as they ride out any reconnect. No give-up.

### Keys: one key per sender

The pairwise link secret cannot cross a forwarder: the forwarder would have to open and re-seal every frame for every receiver.
So forwarded media is sealed with SENDER KEYS:

- **Each sending device derives its media key** from the molecule's current era, the wave id and its own device key, under a context of its own (`PHOTON_WAVE_v1 group sender media`), so no sender key can equal a link secret, an express key or a 1:1 wave's.
- **Every member can derive every sender's key** — every member holds the era — so a forwarder passes sealed frames untouched, and a member opens whichever stream it receives.
- **The frame names its sender** in its clear header (a short device id), so a receiver picks the one key to try instead of trying every member's: trial decryption across fifty senders at 200 frames a second is not affordable.
- **Each sender keeps its own sequence space and step chain**; a forwarder never renumbers. A receiver tracks each sender separately, with the same tag-before-advance rule as today (`packet::open`, docs/group-waves.md "Ingress").
- **A retired era** keeps working for one cutover, as group express already does.

### Who said it

A key every member holds proves membership, not authorship: one member could forge frames as another.
Group express already answers this for signalling with an Ed25519 signature on every frame; at 200 media frames a second per sender that is too costly.
**Signed seconds** (decided, Nick 2026-10-31): each sender signs, once a second, the hashes of that second's frames — audio and video alike — and sends the signature on its stream.
A receiver plays frames as they come and marks a second unverified if its signature fails or never arrives: one signature per second per sender, tiny on the wire, and a forged stream shows as unverified within a second.
It is built after sender keys work (step 2d); until then a stream proves membership only.

### What a forwarder can see

Nothing a member cannot: it is a member, and it holds the keys because it holds the era.
It sees who sends to whom and how much, which every member of a mesh sees already.
A forwarder is never FGTW and never a device outside the molecule.

## Stage 3 — who you hear and see

**Each receiver's work stays bounded however large the group.**

### Levels travel with the audio

Every audio frame's clear header carries its sender's level for that frame (one byte, in stops below full scale — the level the sender's meter already measures).
The level is not secret from a forwarder; it needs it to choose.

### Hearing — every voice

Nick 2026-10-31: "All voices, if possible."

- **Every receiver decodes and mixes every sender** while it keeps up. Decoding a voice stream is a small fraction of a core, and 50 voices down is about 1.6 Mbit/s, so for most groups on most devices this is simply what happens.
- **Keeping up is measured, not guessed**: each party's playout already counts its misses (a frame due with nothing decoded for it). When a device's decoding falls behind — misses rising across parties together, an edge on the counts — it stops decoding the quietest senders first, and takes them back, loudest first, when the misses stop. A dropped sender stays a level on its tile.
- **Forwarders forward every voice**: no ranking at the forwarder, so no quiet voice is ever lost upstream; only a receiver that cannot keep up drops, and only for itself.
- **The mix holds its level**: many voices summed would run into the rails, so the mix passes thru the level plan's own rail shaper (`qgain::cubic_rail`, docs/waves.md) — smooth, invertible, no hard clip.
- **No jumps**: a sender dropped or taken back fades out or in over its frames, on a smooth curve (AGENT.md, calculus or else), and the decision is an edge on the counts, never a timer.

### Seeing

- **Each receiver subscribes** to the streams it will show: the active speakers at full size, the visible small tiles at the thumbnail layer (Stage 4), the tiles scrolled off at nothing. The subscription is a small control frame to the forwarder, sent on change.
- **The stage**: the active speakers large at the top, every other member a small tile below, the grid scrolling when tiles would fall below a minimum size (a fraction of the span, never pixels). Tiles change size along a smooth curve when the speaker changes — never a jump.
- **A tile with no video decoded** shows the face and its level ring, exactly as an audio-only member does today.

## Stage 4 — layers

**Each receiver gets the size its screen and path can take.**

- **Each camera sends up to three encodes at once**: the ladder's own size, a quarter of its area, and a thumbnail (about 160 wide, at a lower frame rate). Each layer is an independent stream with its own sequence space under the sender's key.
- **The forwarder hands each receiver one layer per sender**: the one its subscription asks for, stepped down when that receiver's path is congested — the same congestion verdict the beam ladder reads (docs/beams.md), measured per receiver by the forwarder.
- **Phones**: a second hardware encoder session where the phone has one (the capability check of 2026-10-31, `ENCODER_CAP_W`, extends to "how many sessions"), else the thumbnail is made from the self-view path that already exists.
- **Desktops**: a second software encoder at the thumbnail size costs little; the top layer stays the ladder's.
- Scalable H.264 (one stream with layers inside) is not broadly decodable; independent streams are.

## What does not change

- The 1:1 wave: its engine, keys, ladder, recording and screen.
- Signalling: the lane rows, express, ringing, joining, leaving and the roll call.
- A member is a person, not a contact.
- Each fleet keeps its own recording, as it does today.
- FGTW is a phonebook. No media touches it.

## Recording at any size

Nick 2026-10-31: "Every channel gets recorded, for the video, same idea, we'll have to work on prioritizing for devices and possibly not spool video if it can't keep up, etc. I think we disable the saving and replicating of video by default but enable the audio by default."

- **Every voice is recorded, by default.** Each party's audio is spooled as received — even a sender a slow device has stopped decoding live, since spooling the packets costs no decode — and the keep writes one channel per party, as `merge_group_records` does for the mesh today.
- **Video is not saved or replicated by default.** A setting turns it on; it is a separate setting from audio, which stays on.
- **When video recording is on, video is spooled only while the device keeps up**: the spool writer behind on the video streams drops video spooling for that wave (logged, and said on the wave screen) and keeps every audio channel. Audio always outranks video for the disk and the CPU.
- **Which device records the video** is a fleet question still to work on: a desktop on mains outranks a phone, and a fleet need not spool the same video on every device it owns.
- The video track itself (`beam.video` beside `wave.audio` in the wave container) is not built yet (docs/beams.md stage 6); it comes before any of this video recording does.

## A thousand people

Nick, 2026-10-31: "Do your best to incorporate our changes for molecules so I can get a thousand people in a meeting."
The four stages bound each device's media; a thousand people also needs the parts of a group wave that today grow with the square of the group.

### One media engine per device, not per link

Today every link is a whole engine on its own thread (`wave-engine`), with its own encoder: at a thousand that is 999 threads and 999 encodes on every device.
A group wave instead runs ONE group engine per device:

- **Send**: one capture, one level plan, one encode, sealed once under the sender key (Stage 2), handed to each next hop — the members this device meshes with, or its forwarder.
- **Receive**: one socket path; each frame's clear sender id picks its PARTY in the party table (Stage 1) — that sender's key, step chain, decoder and playout. A party exists only while its sender is heard.
- **Repair per next hop**: the fountain repair and the loss loop stay per path, as Stage 1 keeps them.
- **The 1:1 wave keeps its own engine**, untouched.

### Silence costs nothing

Voice frames today go out whether anyone speaks or not.
With silence detection on the send side (step 3a), a silent frame is not sent; a muted or quiet member sends no media at all.
So a thousand-person meeting carries the voices of the few talking, not a thousand streams: every forwarder, every receiver's decode and every recording channel costs only for the people actually speaking.
This is what makes "every voice" and "every channel recorded" hold at a thousand.

### Signalling that grows with N, not N²

- **No roll call to everyone.** Until v136 every member answered each newcomer on the lane and to every device, so a thousand joins were a million answers; since v136 each member answers the newcomer alone (a thousand joins, a thousand answers each to one device). With forwarders, the forwarders hold who is in the wave; a newcomer asks its forwarder once and gets the joined set in one reply.
- **Joins and leaves travel the forwarder tree**, not express to every device: one frame to the forwarder, passed down the tree. The lane row stays the record and the backstop, as it is.
- **The ring goes once to each member's fleet** over the lane, with express to the forwarders and the members a device meshes with; a thousand express datagrams per signal from one device is not sent.

### Talking at a thousand

A thousand open microphones is a room no one can hear in, however well each voice is carried.
A question for Nick: past a size, does a joiner arrive muted, with a raised hand to be heard, and who unmutes? (Silence costs nothing, so the network does not need it; the people do.)

### The screen at a thousand

The speaker stage of Stage 3: the people talking large at the top, everyone else a small tile in a grid that scrolls; a tile has a minimum size as a fraction of the span, never pixels.
Only the tiles on screen are subscribed to, and only at the thumbnail layer; a tile scrolled away costs nothing.
A face tile with no video is the face and its level ring, the same as an audio-only member today.

## Order of work

| Step | What it is | What it buys | Reaches |
|---|---|---|---|
| 1a | The party table replaces the fixed slot arrays | No eight-device cap | |
| 1b | One group engine per device: one encode, receive by sender id, repair per next hop | CPU per device ~flat in N for sending; no thread per link | Dozens, mesh only |
| 2a | Sender keys and the sender id in the header | Frames that any member can forward untouched | |
| 2b | Forwarder choice from the roll; each sender meshes while its uplink allows | Uplink per sender independent of N; relay media between members | |
| 2c | The joined set from the forwarder; joins and leaves down the tree | Signalling that grows with N, not N² | Hundreds |
| 2d | Signed seconds | Authorship, not just membership | |
| 3a | Silence sends nothing; levels in the header; every voice decoded, the quietest dropped only by a device that falls behind | Every voice heard where the device keeps up; bounded work where it cannot | A thousand, voice |
| 3b | Subscriptions and the speaker stage | Bounded video decode; a screen that works at a thousand | |
| 4 | Simulcast layers | Each receiver gets what its screen and path can take | A thousand, with video |

Steps 1a and 1b are contained and can ship before any of the rest is designed further.
Each step ships on its own and is field-tested before the next: a group wave must keep working at eight while it learns to work at a thousand.

## Decisions (Nick 2026-10-31)

1. **Authorship**: yes — signed seconds, after sender keys work (step 2d).
2. **Forwarders**: chosen automatically, on by default, with "Relay for groups" to switch a device off (device-local).
3. **Mesh or forward**: "whatever the math supports" — each sender meshes while `(N − 1) × stream rate` fits half its measured uplink, and forwards past that; no fixed group size.
4. **Hearing**: "All voices, if possible" — every voice decoded and mixed; a device that cannot keep up drops the quietest for itself only, and takes them back when it can.
5. **Recording**: every audio channel, by default; video saving and replicating off by default, and when on, video spools only while the device keeps up. Which device records video is still to work out.
