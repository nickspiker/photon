# Release notes

Shipped versions are named by their minor number alone (v87); patch numbers belong to dev builds and never appear here.
The top section is always `## Upcoming` and collects what has landed on main since the last release. A deploy renames it to the version it ships as and opens a fresh `## Upcoming` above it.
The app shows the section for the version it runs, and the photon page on holdmyoscilloscope.com shows the newest few.
One sentence per line, however long; plain language for the person who installs it, not for the person who built it.

## Upcoming

- Two phones that ended up on different friendship keys after a re-pair no longer stay silent at each other: the one whose fleet holds nothing newer now runs a fresh pairing itself instead of waiting forever, and messages resume once it completes.
- On a home network a wave now climbs past the top compressed rate to plain uncompressed audio: no codec in the path, a lost packet is a five-millisecond blip rather than added delay, and the recording keeps the full audio.
- A wave answered by a phone that re-keyed the friendship moments earlier now connects: the answer travels under the same key the ring arrived on, so a one-step key mismatch between two phones no longer strands it.
- Dialing right as the offer commits no longer leaves the ring without its keep-alive, which made the other phone stop ringing after three seconds.
- In dozenal and hex, a file or recording size shows as its doubling count in bits, the same rule as the time-ago figure: one byte is four, a kilobyte fourteen, a megabyte two dozen.
## v88

- Nothing you type becomes a link on its own: a web address stays plain text until you press the purple link button beside send.
- On desktop, pasting a web address onto selected text turns that text into a link.
- The link button shortens the address to its plain name, keeps the full address behind it, and hands the name back selected: type a name of your own over it, or press space to keep it; backspace puts the plain address back.
- Links in messages are bold purple, and open only after a confirmation that shows the full address.
- Alerts at the bottom of the contact and attest screens now stack, and the update notice stays until the update is on.
- The update notice only appears while automatic checking is enabled.
- Settings rows wrap at the pane edge under a large font instead of running off it.
- A wave is one entry in the conversation, not two: the outcome and length show at once, and the recording folds into the same card when it is kept.
- A kept wave shows its sound as a two-sided waveform, yours above and theirs below: tap it to play from there, drag to scrub, tap the glyph to stop.
- A pill in the conversation's top bar filters the stream to all, waves only, or text only.
- Zooming holds the content under the pointer still instead of anchoring the page at its top.
- A Dozenal page joins settings, named Dozenal, Hexadecimal, or Arabic for whichever base is on: three pills to pick the base, the why, the digit cheat sheet, and a legend for how long ago things were.
- Hexadecimal is a third base for every number on screen, with plain 0 to F digits.
- In dozenal mode a message's age is one number, how many times a second has doubled since it: Zila Zil ago is an hour, and the Dozenal page lists the rest.

## v87

- Calls on Android run on a new audio path with far lower latency, measured against the phone's own hardware floor.
- An existing conversation can be re-keyed in place without losing anything, and the fleet's ceremony owner is chosen automatically.
- Two devices that had drifted onto different keys for one friend now converge on their own.
- A call no longer drops the instant it is answered.
- A stale copy of a friendship on a wiped device can no longer block repairs for the rest of the fleet.

## v86

- Lock, Wipe, Release and Revoke each mean one thing, and a fleet of one greys out the two that need another device.
- The fleet page names the path a device is reached on: LAN, WAN, direct or relay.
- Buttons wrap like text on every screen, and every number honours the dozenal or decimal setting.
- Fonts are bundled, so the same symbols render on every device and colour glyphs render in colour.
- A message no longer waits a second to send, and a delivered message is always acknowledged.
- Answering a call from the notification, or with the screen off, brings up the call screen.
- Storage no longer reports itself degraded after a full reinstall.
