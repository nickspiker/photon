# Release notes

Shipped versions are named by their minor number alone (v87); patch numbers belong to dev builds and never appear here.
The top section is always `## Upcoming` and collects what has landed on main since the last release. A deploy renames it to the version it ships as and opens a fresh `## Upcoming` above it.
The app shows the section for the version it runs, and the photon page on holdmyoscilloscope.com shows the newest few.
One sentence per line, however long; plain language for the person who installs it, not for the person who built it.

## Upcoming

- The garbled bubbles that read like `photon-callhangup…` are gone, and so are the notifications they set off: behind-the-scenes rows (wave signals, delete requests, key updates, group invites) now carry what they are as labelled fields instead of hidden inside the message text, so they can never again show up as a message or ring you.
- This build cannot exchange messages with an older one in either direction, and attachments and kept wave recordings from before it no longer appear in your history — the old rows are cleared from the device on first launch. Update every device together.
- A new attachment's notification now says what arrived instead of showing scrambled text.
- A device that fell behind now catches up on its own: it could miss messages for hours and only recover when the app restarted, because its catch-up stopped after checking just the newest page of history.
- Catching up rings once per conversation instead of once per page of history, so a device coming back from a long sleep no longer sets off a burst of dozens of chirps.
- Saving messages is much faster on phones: a page of history is written to storage in one go rather than one message at a time, which removes the pauses when a lot arrives at once.
- Finding friends nearby over Wi-Fi Direct no longer silently fails at startup when the phone's Wi-Fi is busy: it waits for the Wi-Fi to settle and tries again.

## v103

- Voice is now called a **wave**, and video a **beam**, everywhere in the app: the ring panel, the notification, the stream filter, the settings and all sixteen languages. The word "call" is gone on purpose, and the About page says why in one paragraph — a call hands your voice to a company to carry, and everyone agrees to pretend nobody kept a copy, which stopped being true when data itself became the storage medium; a wave has no carrier and keeps its own recording, which is yours.
- This build cannot wave with an older one, in either direction, and wave recordings kept before it no longer open. Everything a wave is named by changed at once — the marker its signals ride under, the keys it derives, and the container its recording is written in — so both devices must be on this build or newer. Messages, attachments and everything else are untouched.
- A brand-new phone can get into your fleet again: a fresh device signs with every signature it holds, including one your fleet has not been told about yet, and the check refused the whole envelope over the one signature it had no key for — so the phone could never attest, never declare what it signs with, and never get in ("envelope: eggs do not verify at the required tier"). A signature nobody can check proves nothing, so it is now passed over instead of failing everything beside it; the fleet's own records still demand the full set.

## v102

- The Fleet page shows where your fleet stands on the new signatures: a line at the top names the strength every device has proved and how many have declared the full set, and each device's card says what it signs with — so a machine still to be updated is visible rather than guessed at, and a device you have locked out reads as locked there too.
- A device you lock out is now refused by the fleet itself as well as by the server: the lock-out is written into your fleet's own record beside the server's copy, so the locked device stops counting toward what your fleet can prove and nothing it signs is accepted, even by a device that never reaches the server.
- The Android app now signs with the full set like a desktop does, deriving its signing keys from the same source as its identity, so a phone no longer holds the whole fleet back to the weakest signature.
- A large file dropped on the bridge now arrives: the transport sends at most thirteen pieces at a time to one machine and releases the next as each lands, instead of firing every piece at once — a 345 MB drop fired 1383 pieces into a space with 26 slots, they trampled each other mid-flight and 62 landed. The same window now governs every large send (attachments, history, pairing offers), and a finished send no longer sits in memory holding two copies of itself until something else clears it.
- A file dropped on the bridge shows a progress bar on its row while it crosses: the receiving machine reports how many pieces have landed as they land, the bar fills from that on the sending side and from its own spool on the receiving side, and the "landed at …" line follows once it is whole; the receiver also stops re-reading its entire spool after every piece, which on a large drop was gigabytes of disk reads on the drawing thread.
- On the bridge, pressing Stop no longer wipes the command's output — it adds its verdict at the end, where it belongs — and a command that ends while its last output is still arriving now always gives the prompt back instead of leaving it stuck.
- The Android app's version number matches the photon inside it again: v101's wrapper said 0.100.1 while its contents were 0.101.0, because the wrapper was built from a different copy of the source than the code was.
## v101

## v100

- The Fleet page's status line and a contact's connection line now wear the path's own colour — cyan on the LAN, green across the WAN, amber over the relay — instead of a flat green for anything online, so the word and the colour agree with the device name beside them.
- Opening a conversation no longer waits on the vault: whether an attachment or a wave recording is held here is answered from memory and checked in the background, so a conversation opens at once even while a large wave is still arriving (the phone used to freeze for seconds, and once until Android killed it, while it re-checked all 545 pieces of a recording behind every incoming piece); and a crash on the Android side now writes its own stack into the log before the app dies, so the next "random close" can be read.
- A device of your own that answers is now shown online on the Fleet page from every other device: a presence verdict used to land on only the first row that knew the device, so depending on load order one of your machines could stay grey while another device of yours knew it was up (Leviathan showed the Mac offline while the Mac showed Leviathan online); the same first-row rule hid a proven direct path, so a device of yours could read "relay" here while it read "direct" from the other side; the Fleet page also logs each row's state whenever it changes.
- Your own devices stop re-running their pairing ceremony at every launch: a sibling round always ended in a proof mismatch, because one step of the exchange looked for our half of the handshake under the wrong name, sent a second answer, and then sealed with the second while the other device had already sealed with the first; each side then minted its own era for the fleet conversation and the next launch tore the round down and started again. The round now seals once, on the same secret, on both sides.
## v99

- A phone whose clock is off by more than a minute no longer gets stuck at "timestamp outside valid window": the server's refusal now says whether you are ahead or behind and carries its own clock, photon adopts that as its time base on the spot and sends again — for log submissions and for attesting alike — and the server keeps a note of every such refusal so the cause can be read later.
- Keeping a wave no longer re-encodes it: the recording is the packets the wave actually carried — your side as it was archived during the wave, the other side exactly as it arrived plus whatever was filled in afterwards, a lost moment left silent rather than guessed — so a long wave keeps in seconds instead of minutes on a phone, and a wave card with two recordings of the same wave now shows the one this device holds instead of asking to fetch it.
- The picture viewer is opsin's, whole: open any image — DNG and RAW included — and you get opsin's image area with pan and zoom, the tool panel (navigator, 1:1 and Fit, Save, Info, rotate, crop, the raw histogram with its scale and clip pills, the exposure slider, the chromaticity chart), the frame-info readout, and opsin's keys; the Back pill at the top-left, Escape, or the phone's back gesture leaves it; the old three-tier viewer with its half-stop pills is gone.
- A pigeon is now fetched from one device at a time (the friend's devices with a proven direct path first, then their other devices, then your own), instead of asking every device at once and having each of them send the whole file; a served pigeon rides the relay only when the direct path to the asker is unproven; and a pigeon already held here ignores late copies of its manifest and chunks — together the re-uploads and re-downloads that showed up as repeated transfers of the same pigeon.
- Android's glass corners are a tenth smaller, and a release build draws no hairline along the glass — the corner beyond the curve is plain black.
- A desktop or a phone on home Wi-Fi now asks its router to forward photon's port (NAT-PMP, PCP or UPnP, whichever the router speaks) and publishes the forwarded address, so friends reach it directly with no punch at all and the relay stays idle; on cellular there is no router to ask and nothing changes.
- When a friend's presence ping arrives by way of the relay, photon punches toward every address that friend has published at that instant — inside the window the friend's own punch just opened — instead of waiting for its own next presence cycle; two home routers that never lined up now get a coordinated open every ten seconds.
- A network move on a laptop that failed to re-ask the directory for its public address now says exactly which piece was missing, and the re-ask logs when it starts.
- Android's glass corners are twice as deep, so the small corners sit inside the phone's glass arc instead of on top of it.
- The Wave pill and a wave card's "wave back" now say "no direct path" and stay dim when a friend is reachable only through the relay — a wave placed there would connect and carry nothing — and flip back the moment a direct path opens.
- A friend you can only reach through the relay now gets punched at from your side too: the address lookup that turns a published address into a working direct path used to run only while a brand-new contact was still connecting, so an established friend behind a home router could stay relay-only forever from one end. And the presence ring is honest about it — green means a proven direct path; a friend reached only over the relay reads amber until the punch lands.
- Android lays out under the status bar now that it draws edge to edge: the orb, the back arrow, the wave pill and every settings header start below the bar (and the orb sits in from the corner by the same height — on desktop by the height of the window controls); the window corners keep photon's own squircle, sized so the small corners sit just inside the phone's glass arc and the big ones at twice that.
- Android draws edge to edge now — under the status bar and the gesture pill — so the window's corners are the phone's glass corners: the small corners follow the glass arc exactly and the big ones twice it, the same proportion the desktop window wears; the compose box keeps clear of the gesture pill, and for now the perimeter hairline stays visible so the fit can be checked by eye.
- Photon's own clock now keeps counting while the phone sleeps: it had been following a clock that stops during suspend, so after a long sleep every stamped request ran minutes behind and the server refused log submissions ("timestamp outside valid window") — the same message a phone with a genuinely wrong clock sees.
- A phone whose clock is off by more than a minute can attest and submit logs again: the requests are stamped with photon's own time, which the app already keeps independent of the system clock, instead of the clock the banner is warning you about.
- Android touch is honest about gestures: a pinch no longer snaps to whichever finger lifted last and jerks the screen, and once a finger has scrolled or zoomed nothing gets selected on release — a tap is a tap only if the finger stayed put.
- On a link so slow that a repaired packet would arrive too late to play, the wave stops asking for live repairs and lets the recording collect them at the end instead — the repair traffic was drowning what little bandwidth such a link had.
- The twelve dozenal digits are typeable: in the dozenal base a row of glyph keys sits above the message box while you compose, a tap inserts one, and the Base page has a pill that copies all twelve to the clipboard for pasting anywhere in photon.
- Settings has a Wave page: this device's hearing (an earpiece trim in stops on a bar with a tick per stop, six stops each way, quiet-min to loud-max, live during a wave — one stop doubles or halves what this device plays, for phones whose earpieces sit outside what the rocker can reach — with the route and rocker beside it), this device's voice (what each microphone's calibration has learned, a forget pill, and a "measure now" that listens to a sentence and stores the result), the last wave's stats in plain words (path, round trip, loss and recovery, the rate ladder, the level plan's aim), and the wave preferences — ring and vibrate on incoming, hold every wave here, and whether raw audio may be tried beyond the local network.
- Before you attest, the orb's Security page now offers Wipe: a phone that was someone else's, or a handle nobody will type again, can be cleared to a blank slate without the old handle, and the hint says what stays true afterwards.
- The Base page, with arabic numerals selected, now explains what base ten is: where the numerals came from, how recent zero is, that other bases exist, and why photon's magnitudes are counted the way a slide rule multiplies.
- WebP pictures open in the colour-managed viewer with live exposure, and the viewer recognises every picture by its bytes rather than its file name.
- The wave's self-aim correction now fires at the same threshold as its first step, so an energetic greeting or a hesitant first word can no longer leave a wave a few decibels off for its whole length.
## v98

- Atoms and molecules: tap New atom on the contacts screen to found a conversation that is yours alone, titled or not, and it is an atom; open its panel and press Create a molecule! to offer a friend a bond over the conversation you already have with them, and the moment they tap Bind it is a molecule — the same thing under a different count, the way a lone hydrogen becomes H₂.
- Nobody owns a molecule, nobody can bind you into one, anyone bound in can bring in anyone they know, and only you can remove yourself; the title is a label anyone can change, the real identity is a random number minted at founding, so a name can never be squatted, sold, or stolen.
- Every message in a molecule is encrypted once on your own lane and sent to every member, each member's device holds its own key bundle, and leaving posts your signed departure while the others mint a new key you will not hold.
- The contacts screen filters among friends, atoms and molecules; Settings has a Conversations page that explains all of this in plain words and holds the default for what newcomers may see.
- The language list grew to sixteen, and every one of them speaks atoms and molecules.

- Fewer tiny gaps on a busy Wi-Fi: the playback buffer no longer sheds the small bursts of early-arriving audio that home Wi-Fi delivers (it was shedding them, then running dry a moment later), and the loss loop now aims below its ceiling instead of sitting exactly on it — a few more milliseconds of buffer for a much steadier voice.
- The self-aim's first step needs a plausibly loud voice before it trusts itself, so a quiet first word can no longer set the level hot for the next twenty seconds.
- The wave's self-aim waits for real speech: it needs four seconds of sound that moves the way a voice does before it trusts a measurement, so breath and handling before the conversation starts can no longer aim the level wrong, and it allows itself one correction later if the first aim proves badly off.
- After a Wi-Fi stall the backlog of the other person's voice is shed from the pauses, not from the words — before, a few seconds of stall could cut whole phrases out of what you heard.
- A wave answered while the ringback was still winding down could go out silent — your side heard them, they heard nothing — because the ringback's cleanup and the wave's start fought over the microphone; the session now has one owner at a time and a refused microphone open is retried.
- Opening a conversation asks the contact if they are there and gives them one second: no answer and the header shows offline right away, flipping back the moment they respond.
- The wave's ear for "is this speech?" is now relative to the room: it tracks this wave's own quiet and counts only sound well above it, so background noise can never be mistaken for a voice and crank the send level, and the quiet itself — measured, not the manufacturer's claim — seeds the level for a microphone the app has never heard speak.
- A wave now aims its send level by listening to itself: a few seconds in, once it has heard enough of your actual voice, it corrects its gain once to put you exactly on the plan — so a quiet posture no longer makes you inaudible and a stale calibration no longer makes you crackle, whatever the last wave measured.
- The volume readout the wave's diagnostics rely on now reads the control that actually governs the wave's loudness, as a plain fraction of full — vendor loudness curves had it reporting near-mute on phones that sounded fine.
- The crackling wave is fixed: a voice calibration learned from quiet test waves could drive the send level far past the plan and pin every syllable against the ceiling — the gain is now capped, and a calibration that a real wave proves badly wrong is replaced on the spot instead of nudged toward the truth over many waves.

## v97

- A wave on Android now starts on your headset when one is connected — wired first, then Bluetooth, then the earpiece — instead of ignoring it, and a new pill on the wave screen shows the device that is playing and cycles through the available outputs when you tap it.
- Waves connect away from home: when your phone changes networks — leaving the house, dropping to cellular, carrier-shared addresses — it re-learns its public address right away, and again at the moment you dial if it still has none, where before both sides could sit silent for half a minute aiming at a dead address.
- Waves ride congestion by watching delay the way it actually behaves: a steadily climbing round trip means the link is over capacity, so the wave drops its rate in one jump to just under what the link measured, and it will not climb again until the delay flattens out.
- The two ends of a wave now tell each other what actually arrived: your send rate answers to the far side's losses instead of your own, loss with no delay building behind it is treated as radio noise rather than congestion, and raw uncompressed audio only runs after a clean loss-free stretch and steps back down the moment the listener starts starving.
- Settings has a Vault page: capacity, the space occupied with its live and reclaimable split, entries held, the lifetime write odometer, commits survived, and a plain health verdict.
- Seeking works on a wave that is selected and playing — press picks the playhead up, release plays from there.
- Sending a picture shows its progress: the preview wipes in from the left as the pieces land, and the bar tracks the whole transfer instead of repeating one piece's.
- The keyboard gets out of the way when a wave or beam rings in.
- The screen blanks at your ear and only at your ear: on the earpiece the proximity sensor turns the display off, on speaker or headset it never does, and answering no longer leaves the screen fighting to stay lit for the rest of the wave.
- Desktop launch no longer hangs while attachments settle into the vault — the two-minute freeze on a large vault is gone.
- Groundwork for group conversations — one shared root delivered over the friendships you already have, membership as signed records — is landing under the hood; nothing visible in the app yet.

## v96

- Waves connect the instant they are answered: your voice flows from the first captured frame, and an answer now always reaches the origin (it rides the relay like the ring does), so "answered on one side, silent on the other" is gone.
- The level plan, the way the telephone network ran it: the microphone is read raw at 24-bit and calibrated per device and per input (the calibration follows your device through your fleet settings, so it survives a reinstall), scaled once by a fixed constant, and a shout rounds smoothly into the ceiling instead of crackling. Every wave transmits at the same known loudness; the volume rocker on the listening phone is the only adjustment, and it governs the wave while it rides the earpiece.
- Nothing touches your voice on the way out: no gate, no ducking, no automatic level. Echo is handled on the listening side — while your mic is hot the other person is turned down in your speaker for that moment, the speaker is held where their echo stays under −36 dB, and the far room's quiet is quiet. None of it is recorded; a kept wave holds every party as they sounded.
- Raw PCM ("plaid") runs wherever the path has headroom, not only on a LAN: a wave climbs to it while the round trip holds steady and steps back to the codec at the first sign of loss. Same-room waves now run it in both directions.
- Audio arithmetic is exact: every gain in the playback path is one integer scale that carries its remainder to the next sample, so quiet audio keeps its bottom bits and a gain of one is provably untouched. Image previews fold the same way.
- A lost stretch of audio fades out and fades back in instead of clicking at either edge.
- Android remembers your zoom. The pinch scale you set was saved but never applied on the next launch; it is now.
- The top-left orb is half again as large; the text beside it is unchanged.
- A wave on the timeline is its waveform and nothing else, hairline to hairline. Tap it to select it, and the options appear: play, wave back, beam back, export, replicate, discard. Playing takes those two taps and never starts from a scroll or a stray touch on the band.
- Playing a wave you recorded on this very device no longer says "fetching from your devices".
- A wave that is already carrying audio keeps its working path: a moment of silence no longer makes either side re-aim at a different address, and an address push that names the address already in use is ignored instead of bouncing between the two phones.
- Opening the vault is instant again: it reads the index and nothing else, where it had been reading every kept wave in full at every launch.
## v95

- Waveforms are rendered a new way, everywhere they appear: every stored sample lands in its screen column at its own height, and the edge pixels are shaded by how many of them reach that row, so the contour is smooth at any width with nothing invented and nothing thrown away.
- Waveform colour is now the balance of three tuned voice bands — warmth below 240 Hz reads red, presence around 2.4 kHz reads green, sibilance and air above 8 kHz read blue — as vivid in a whisper as in a shout.
- A wave now records your voice clean, before echo ducking touches it, and the ducking itself is saved as data beside it; the other side's card gets your true waveform, not the ducked one.
- Keeping a wave is near-instant now: the recording is written once during the wave at archive quality, so the keep packages it instead of re-encoding it, and recordings take a fraction of the space they did.
- A wave's waveform card colours in seconds after hangup on both sides — the envelope travels ahead of the audio.
- Drop a song into any conversation and the row IS its waveform, left channel up, right channel down; tap it for the options and the play button, tap play to listen, tap along the band to seek while it plays.
- Pictures and songs travel without their filenames; a picture is the picture and a song is its waveform, and nothing about your camera or your files rides along.
- On the desktop, opening a picture opens it in opsin, with exposure and the full raw pipeline; the in-app viewer stays for phones.
- The viewer is a real full screen now: while it is open the conversation underneath stops being drawn entirely, which also ends the black-rectangle open.
- The reaction buttons and the option buttons are dark with bright labels, as they were always meant to be, instead of blinding.
- A wave no longer appears twice on the timeline when your other devices lived the same wave.
- Old kept recordings and their previews from before this version do not play or draw; the recording format changed for all of the above and takes no baggage along.
- The wave screen's ring reads cyan for a same-network wave that runs over IPv6, not just over a private IPv4 address.
- When the other side's phone changes network mid-wave, yours now sends its own address straight back and tries every address it knows for them, so a wave can pick up again on mobile data instead of dropping.
- The earpiece route is kept only where it keeps the low-latency audio path; a phone whose vendor audio policy claims voice streams falls back to the loudspeaker at full speed rather than the earpiece at 40 ms.
- Waves on Android play through the earpiece, the way a telephone earpiece does, while keeping the same low-latency audio path; the volume rocker adjusts the earpiece during a wave and media the rest of the time.
## v94

- Presence pings to a friend with several devices are now booked against the device that owns each address, so a good answer from one of their other devices no longer counts as a mismatch and gets re-asked forever.
- Waveforms are drawn the way Lumis draws its histogram: folded at four sub-columns per pixel and lit by coverage at the tip, so the bars are anti-aliased in both directions instead of stair-stepped.
- The option buttons under a message are now near-black, each with a hint of its verb's colour.
- The "photon isn't responding" prompt at launch is gone: the vault now opens on a worker thread while the launch screen stays responsive, instead of holding the main thread for several seconds as the vault grew with kept waves.
- The wave screen now repaints once a second while a wave runs, so the timer, the live stats and the path ring keep up; the ring also reads cyan on the same network as it should, where it stayed green before.
- The stream filter button no longer hides behind message rows.
- Tapping beside an attachment opens its details: name, type, size and dimensions with the time up top, and reply, save and delete below, every option a real button.
- An attachment row is now just the attachment: the picture, the first lines of a code or text file, or the waveform, with no size or hint line. Tap the picture or the code to open it; tap the row beside it for reply, save and delete.
## v93

- Fixed a crash on Android that could hit right after answering a wave, when a background network event ran the app's tick on the wrong thread.
- An attachment or wave that nobody answers for is asked for again every twenty seconds instead of showing "fetching" forever.
- Waveforms are brighter and truer: bar height is the actual loudness against one fixed scale for everyone, and colour is the balance of low, middle and high frequencies in the voice, vivid whether the moment is quiet or loud.
- A wave that has already ended can no longer ring again when its original ring arrives late over the relay.
- A hangup, a ring and a reconnect signal now travel on the path the wave's audio is using, and always carry a relay copy, so the other side hears you hang up and rings when you wave back even when your address book points at a different one of their devices.
- The wave screen's avatar now wears the presence ring in the colour of the path the wave is actually on: cyan on the same network, green across the internet, amber while it has no direct route yet.
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
- Wave signalling refuses a repeated or stale frame, so a copy captured off the network cannot be replayed to redirect a wave.

- A wave now aims at the device that answered rather than at whichever of that person's devices was easiest to reach. A wave with someone's phone was being sent to their laptop, which is why a wave could show a healthy green ring and carry no sound at all.

- Waves now work between a phone on mobile data and a phone on home wi-fi. A device on a home network was telling everyone its public address was its home address, because the only devices watching it were on that same network, so a friend on mobile data had nowhere real to send audio. Messages still got through, which is why this looked like a wave-only fault.

- When your phone changes network it now tells the friends who could not find you, not just the ones who already could, and they answer with where they are, so a wave between mobile data and home wi-fi has both sides' real addresses after one exchange.
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

- Waves on Android run on a new audio path with far lower latency, measured against the phone's own hardware floor.
- An existing conversation can be re-keyed in place without losing anything, and the fleet's ceremony owner is chosen automatically.
- Two devices that had drifted onto different keys for one friend now converge on their own.
- A wave no longer drops the instant it is answered.
- A stale copy of a friendship on a wiped device can no longer block repairs for the rest of the fleet.

## v86

- Lock, Wipe, Release and Revoke each mean one thing, and a fleet of one greys out the two that need another device.
- The fleet page names the path a device is reached on: LAN, WAN, direct or relay.
- Buttons wrap like text on every screen, and every number honours the dozenal or decimal setting.
- Fonts are bundled, so the same symbols render on every device and colour glyphs render in colour.
- A message no longer waits a second to send, and a delivered message is always acknowledged.
- Answering a wave from the notification, or with the screen off, brings up the wave screen.
- Storage no longer reports itself degraded after a full reinstall.
