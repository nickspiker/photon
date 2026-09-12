# Release notes

Shipped versions are named by their minor number alone (v87); patch numbers belong to dev builds and never appear here.
The top section is always `## Upcoming` and collects what has landed on main since the last release. A deploy renames it to the version it ships as and opens a fresh `## Upcoming` above it.
The app shows the section for the version it runs, and the photon page on holdmyoscilloscope.com shows the newest few.
One sentence per line, however long; plain language for the person who installs it, not for the person who built it.

## Upcoming

## v94

- Presence pings to a friend with several devices are now booked against the device that owns each address, so a good answer from one of their other devices no longer counts as a mismatch and gets re-asked forever.
- Waveforms are drawn the way Lumis draws its histogram: folded at four sub-columns per pixel and lit by coverage at the tip, so the bars are anti-aliased in both directions instead of stair-stepped.
- The option buttons under a message are now near-black, each with a hint of its verb's colour.
- The "photon isn't responding" prompt at launch is gone: the vault now opens on a worker thread while the launch screen stays responsive, instead of holding the main thread for several seconds as the vault grew with kept waves.
- The call screen now repaints once a second while a wave runs, so the timer, the live stats and the path ring keep up; the ring also reads cyan on the same network as it should, where it stayed green before.
- The stream filter button no longer hides behind message rows.
- Tapping beside an attachment opens its details: name, type, size and dimensions with the time up top, and reply, save and delete below, every option a real button.
- An attachment row is now just the attachment: the picture, the first lines of a code or text file, or the waveform, with no size or hint line. Tap the picture or the code to open it; tap the row beside it for reply, save and delete.
## v93

- Fixed a crash on Android that could hit right after answering a wave, when a background network event ran the app's tick on the wrong thread.
- An attachment or wave that nobody answers for is asked for again every twenty seconds instead of showing "fetching" forever.
- Waveforms are brighter and truer: bar height is the actual loudness against one fixed scale for everyone, and colour is the balance of low, middle and high frequencies in the voice, vivid whether the moment is quiet or loud.
- A wave that has already ended can no longer ring again when its original ring arrives late over the relay.
- A hangup, a ring and a reconnect signal now travel on the path the wave's audio is using, and always carry a relay copy, so the other side hears you hang up and rings when you call back even when your address book points at a different one of their devices.
- The call screen's avatar now wears the presence ring in the colour of the path the wave is actually on: cyan on the same network, green across the internet, amber while it has no direct route yet.
- A phone that changes network mid-wave now tells the other side straight away, from Android's own network signal and from the first five seconds of silence, instead of ringing a dead address for thirty seconds.
- The doubling scale is now a pure logarithm: one second, one bit, one wavelength read Zil, and every age and size reads one digit lower than before (an hour is Stelor, a byte Ter, a megabyte ZilaStelor). Nothing reads as a word: "now" for an age, "empty" for a size.
- Hexadecimal is now linear everywhere: every timer, age and duration is a plain seconds count, a round trip is seconds with a hexadecimal fraction, and sizes are the bit count, so nothing on that base is scaled or split into minutes.
- The Base page now explains the scaling in plain words: what one number for how much means, why doublings, what "one" is on every scale, and a length legend from a hair to the observable universe, all in doublings of the hydrogen line's wavelength.
- A wave that starts without a route now exchanges addresses on its first tick instead of waiting on the presence cadence; the first mobile-to-home wave connected in the field but took 18 seconds to do it.

## v92

- Keeping a wave after hangup no longer crawls when the phone dozes: the phone stays awake for the transcode, which also runs lighter. A thirteen-minute wave that took 53 minutes to keep now takes about a minute.
- The Diagnostics log size reads in the current base like every other size.

- Kept waves now replicate to your other devices in pieces with resume, like other attachments; a 26 MB recording sent as one transfer never arrived at the desktop. Recordings kept before this update are re-packaged the first time a device asks for them.

- The ring around an avatar is thick enough to read at a glance, in the colour of how you are connected: cyan in the same room, blue radio-direct, green across the internet, amber relayed, and absent when offline.
- A wave in progress shows its own numbers on every build: the speed rung by name (sublight, light speed, ridiculous speed, ludicrous speed, plaid), the round trip, the loss out of 256, and the buffer depth, refreshed every second.

- A wave now tries to survive a network change: when audio stops arriving, each side aims its audio at the other's other known addresses in turn and sends its "I'm here" signal everywhere including the relay, instead of talking to the address that just died.

- When your phone changes network mid-wave it now tells the person you are talking to directly, over the relay, and their audio re-aims at once instead of hunting for you.
- Call signalling refuses a repeated or stale frame, so a copy captured off the network cannot be replayed to redirect a wave.

- A wave now aims at the device that answered rather than at whichever of that person's devices was easiest to reach. A wave with someone's phone was being sent to their laptop, which is why a call could show a healthy green ring and carry no sound at all.

- Waves now work between a phone on mobile data and a phone on home wi-fi. A device on a home network was telling everyone its public address was its home address, because the only devices watching it were on that same network, so a friend on mobile data had nowhere real to send audio. Messages still got through, which is why this looked like a call-only fault.

- When your phone changes network it now tells the friends who could not find you, not just the ones who already could, and they answer with where they are, so a call between mobile data and home wi-fi has both sides' real addresses after one exchange.
- A phone on home wi-fi no longer mistakes its home address for its public one, and forgets its old public address when it changes networks.


## v91

- An idle screen on Android no longer spends a third of a core keeping itself awake: the app's housekeeping ran at every display refresh, and several pieces of it (a socket lookup, a key derivation per contact, a peer-list scan) were needlessly repeated 119 times a second. They now run once, and an idle screen asks for frames at a slower cadence until you touch it.

- Kept waves now record your microphone as captured, before the echo canceller and the ducking, and store each party as its own full-quality Opus stream, so a recording is one clean generation from the raw source rather than a re-encode of what went over the wire.
- The hangup exchange that fetches the last stretch of a wave from the other side now waits for the other side's fresh count and says goodbye before leaving, so both recordings get their final second instead of one side timing out.
- The wave histogram averages in the log domain and is folded once per width instead of every frame, which removes the spikes and the drag the histogram put on scrolling.

- The wave histogram's unplayed part is now solid at half brightness and playing simply brightens it; the translucent ghost and the floating edge pixels are gone.
- Samsung phones no longer redraw and post the screen on every idle frame.

- A wave to a friend you have no direct path to now rings: the ring signals ride the relay when no direct route exists, and a ring that arrived by the message lane no longer gives up after three seconds waiting for direct beats. Three rings in a row failed this way in the field.
- Leaving a conversation, or switching to another one, stops a wave that was playing.

- Opening a picture now renders it through opsin's colour pipeline: camera RAW and DNG with their own colour matrices, JPEG XL, JPEG and the rest, all as one linear image in photon's colour space. An exposure row under the picture moves it by half stops, resets it, or shows clipping, live.

## v90

- A wave's buffering now follows the actual loss on the link instead of ratcheting up on every stumble: it settles as low as the link allows, holds loss to about one packet in two hundred and fifty-six, and fades over the rest. Every packet also measures the round trip, so the log says how far the other end really is.
- A conversation holding attachments or kept waves draws at full speed again: the screen used to ask the vault about every file on every frame, which cost most of a second per frame with a few recordings in view.
- Pictures show their true colours on Android (red and blue were swapped in previews and the viewer).
- A held picture, text file or kept wave now offers Save beside Open or Play, so downloading and viewing are separate taps.

- A kept wave no longer carries the link's holes: every packet you lost is asked back from the other side while the wave runs, and the last stretch is fetched right after hangup, so the recording is whole even when the live sound stumbled.
- The wave card's histogram is logarithmic now, with the other party above the line and you below it, each scaled to their own loudest moment.

- Security gains Kill: one tap drops your identity and ends photon on the spot, nothing deleted; launch again and type your handle to come back.
- A native crash on Android now reports where it died: the crashing thread's frames are decoded from the system's crash record at the next start, the last minutes of the log are saved as the process dies, and the fault handler is armed once the log folder is known, which it never was before.

- The log keeps its last half minute even when Android kills the app outright: the in-memory batch writes through once it is thirty seconds old, so an unresponsive-app kill no longer erases what led up to it.
- Installing an update no longer copies the package on the screen thread, which could hold the app unresponsive for seconds right after the install prompt.

- Starting photon on a phone no longer stalls for five seconds: the vault's two mirrors were being compared block by block at every open, which grew with every kept wave and was what tripped Android's "not responding" prompt. Mirrors that already agree skip the comparison.
- The attach button wears a dove for now; attachments may become pigeons.

- When the screen keeps redrawing with nothing changing, the log now says so every five seconds and names what asked for the redraws, so the next report can point at the cause.

## v89

- Attachments know what they are: a picture, a camera RAW, a video, a sound, text, code, an archive, a program or a document, read from the file's own bytes, with a matching mark on the bubble.
- A picture shows a small preview on the bubble the moment it is sent, on every device, before the file itself arrives; a sharper preview follows, and tapping opens a viewer with zoom, pan, arrows between pictures, the full-size original on request, and Save. Camera RAW files (DNG and the common makers) and JPEG XL preview too.
- Text and code files show their first lines on the bubble and open in a reader.
- Files of any size send: large ones travel in pieces, a broken transfer resumes where it stopped, and saving never holds the whole file in memory. The old 25 MB limit is gone.
- Programs and archives never run: they save, and the bubble says what they are.
- A re-pairing started by a friend now finishes on every one of your devices, not only on theirs: a sibling's routine chain sync during the exchange no longer marks the friend as done and quietly cancels the finish, which was what left the two of you on different keys in the first place.
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
