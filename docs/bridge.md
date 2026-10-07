# The bridge — your devices, reaching each other

The **bridge** lets one of your devices work another directly: run commands on it, drop files into it, and eventually see its screen, hear its mic, read its sensors, and drive its input. It replaces SSH, RustDesk, "find my phone", AirDrop and KDE Connect with one thing that needs no password, no key file, no open port, no daemon, no account and no vendor.

It is **passless** because it has nothing to add. The two devices are already members of the same fleet: each one proves it is you every time it speaks, and every frame between them rides the fleet's own encrypted lanes. Being a member is the credential. A second secret on top of that would only be one more thing to phish, leak or forget.

**Status:** the shell and file drops (client to host) are SHIPPED; the headless lifeline that keeps a host reachable after its display dies is SHIPPED. Everything under [Where it goes](#where-it-goes) is design. The cross-device leg of pigeons is field-pending.

## Words

- **Host** — the device being worked. **Operator** — the device you are holding. Any member can be either, and both at once toward different siblings.
- **Sibling** — another member device of your own fleet. The bridge exists between siblings and nowhere else.
- **Pigeon** — a file carried across the bridge. A dropped pigeon flies operator → host. A **homing pigeon** (not built) is one the host sends back when asked.
- **Verb** — one typed thing the bridge can do (run a command, land a file, ring, locate, share the screen). The shell is one verb among many; it is not the bridge's foundation.

## Laws

These hold for every verb, shipped or future.

1. **Members only.** Host and operator are both owned members of one fleet. A proxy (a leased or airport device, docs/device-lease.md), a husk, a locked-out device, a friend and a molecule are never on either end of a bridge. Removing or locking out a device ends its bridge at the next frame, because the next frame no longer opens.
2. **Passless.** Fleet membership is the credential. No verb ever asks for a password, PIN or token. Where the operating system insists on its own consent (macOS screen recording, Android MediaProjection, a Wayland portal), you grant it once on the host and it is remembered, never asked again for each session.
3. **The host decides what it offers.** Each verb beyond the shell has an on/off switch on the host's own Security page, set by hand on that device and never synced from another member. A member that is compromised can use what a host offers; it can never widen what the host offers. The **live verbs** (screen, input, mic, camera, location) default OFF.
4. **Live verbs are visible on the host.** While anything is watching, listening, locating or driving, the host shows who: an indicator naming the operating device, for as long as the verb runs. It is your device, but the person holding it might not be you: a family member at your desk, or you tomorrow wondering whether something is still listening. The honest indicator is the price of having no password.
5. **Typed, never text.** A row does something only because of its typed reference (`RefKind::Bridge*`) and its typed `BridgeWire` fields. The words in a row are for a human to read and nothing more. An untyped row never runs, whatever it says (AGENT.md "Text Carries Only Human Words"; the old transition arm that ran any plain sibling row was removed 2026-09-25).
6. **Ephemeral.** Bridge rows are never persisted. Opening a session wipes the screen at both ends. Anything you want kept, you keep on purpose: a pigeon lands as a file, and a screenshot can be saved as an attachment. The bridge itself remembers nothing.
7. **No identity verbs.** The bridge never signs as the host's identity for fleet operations: no device add, departure, countersign, lockout, release or custodian act. Those have their own ceremonies, and they stay there. (A shell can still `rm -rf` the host's disk. That is your right over your own device, not the bridge signing on your behalf.)
8. **Edges, not timers.** A verb reacts to events: output arriving, an ACK, a sensor's own change callback, a Stop press. There is one granted timer, the shell's pacing of at most one output delta per second. No new verb gets a timer without the same explicit grant.

## What ships today

### Opening a session

Go to Settings → Fleet and tap **Bridge** on an online sibling's device card (`open_bridge_conversation`, ui/photon_app/bridge.rs). Siblings are contacts carrying `is_sibling`, and a sibling's conversation *is* the terminal: everything you type there leaves as a `RefKind::BridgeCmd` row (messaging.rs). No `$` prefix and no sniffing are involved.

Opening a session starts fresh at both ends:

- The operator wipes the rows on screen. If frames from an old session are still in flight, it retires them by **rotating its lane** (`rotate_our_lane`), never by clearing pending messages directly.
- It sends a hidden `BridgeReset` row, and the host drops its shell for this sibling and rotates its own lane the same way. The next command starts a new shell in home.

### Commands

**Host platforms:** unix desktops (Linux, macOS). **Operator platforms:** every platform, including phones.

- **One persistent bash per sibling**, owned by one worker thread per sibling (`BridgeShell`, `spawn_bridge_worker`). So `cd`, exported variables and shell state persist like a real terminal, and one sibling's long build never queues another sibling behind it. `exec 2>&1` merges stderr into stdout. A random sentinel for each session marks where a command's output ends, along with its exit code and cwd. That sentinel is plumbing between photon and its own child process and never touches the wire.
- **Runs on first receipt only.** A command runs when its row is new. Rows that are re-served, duplicated or backfilled from history never run again.
- **No timeout.** Busy is not wedged: a command is alive while its process exists, and you hold the interrupt. An old 30-second reset lied about killing the command and orphaned a running deploy (field 2026-08-23), so it is gone.
- **Output streams as deltas.** `BridgeOut` rows reference the command's eagle_time and carry only the new bytes (`BridgeWire.delta`). The host spools unsent output, capped at 64 KB. If the cap trims the front, the next frame's elision marker counts the trimmed bytes, so a gap is always explicit. At most one delta per sibling is in flight, and the next waits for that delta's own ACK. That ACK is the wake edge, and a parked spool keeps accumulating, so nothing is dropped. "Finished" is a field: the exit code rides whichever delta comes last. A quiet command sends zero frames.
- **Locus.** Every output frame names the host machine and the shell's cwd (`BridgeWire.host`, `.cwd`), so the operator always knows where the command ran. A pull meant for photon once ran in keys/ because nothing showed that.
- **Stop.** Each press sends a `BridgeCtl` row that carries a signal and escalates SIGINT → SIGTERM → SIGKILL. The host signals the command's process tree (`bridge_child_tree`) and never bash itself, so the session and its cwd survive. If a Stop finds nothing running, the host still answers with a verdict. The third press releases the prompt locally whatever the host does (a host that is restarting may never answer).
- **Stream loss.** If the host is judged offline while a command is in flight, the operator stamps that command's row closed with a notice (`bridge_watch_stream_loss`). A deploy that restarts photon on the host kills the stream, even though the command itself runs on.

### Pigeons (files into the host)

Drop a file on an open bridge and it lands in the directory the host's shell is standing in (docs/PT.md, "Pigeons, v2"):

- The operator stores the file in the vault and announces it with a typed `BridgePigeon` row. The row's text is the file name; `BridgeWire.pigeon` carries the name, the hash of the whole file and its size, and **never a path**. Only the host knows where its shell actually stands, and a wire that cannot name a destination cannot write outside the directory you are looking at.
- The bytes travel as `pigeon_chunk` PT frames sealed under the fleet key, each its own lettered stream with at most thirteen in flight to one peer at a time. Every frame carries the file's size and its name sealed under the fleet key, so whichever frame arrives first opens the host's spool — the announcement row and the chunks may land in any order. The host writes them into a spool on disk that is its own receipt record (a zeroed window marks a missing chunk), so a partial transfer resumes after a restart and stays out of RAM.
- The host repairs: when a pigeon resumes, and whenever one stops moving for twenty seconds, it asks the operator's device for exactly the chunks its spool lacks (`pigeon_want`, read off the spool at the moment of asking). After eight unanswered asks it says so in the transcript and keeps the spool for the next session.
- When the spool is complete, the host decrypts it, checks the hash of the whole file and lands it with `land_blob`, **replacing** any file with the same name. It adds no `(2)` suffix and no backup copy. Undo is the host's filesystem snapshots. The host then answers with a `BridgeOut` row naming the path.
- The host acks landed chunks in a thinned way (`pigeon_ack`, at most about 65 per pigeon), and those acks drive a progress bar on the operator's row.
- The operator keeps its copy until the host reports every chunk held, so a lost chunk can always be re-sent; then it sheds it (unless the vault held those bytes before the drop). The host sheds the spool once the file has landed.

### Headless lifeline

A host is only useful while photon is running on it. `photon --lifeline` runs the network core and the bridge with no display. It is enrolled from the **bulletproof bridge** checkbox on the Security page (a systemd user unit on Linux, a KeepAlive LaunchAgent on macOS), and when X dies, the running app drops into it within the same session. See docs/headless-lifeline.md.

### Invariants learned the hard way

- **Sibling lanes are anchor-only.** The braid weaves each row against the rows before it. With ephemeral rows plus woven strands, the receiver misses a strand, holds the frame forever and never ACKs it. Ephemeral rows are only safe because sibling lanes weave nothing.
- **Never clear pending messages directly.** Each frame links the previous frame's hash. Clearing mid-chain leaves a hole the peer buffers behind forever. To abandon frames, retire the lane with `rotate_our_lane`.
- **Gate a stream on its own ACK, never on the lane's pending count.** Fleet-sync chatter on the same lane starved the output feed (the silent v82 deploy: 52 seconds of cargo output and the operator saw nothing).

## Where it goes

The bridge grows by adding verbs, not features. Each verb rides one of three **planes**, the same split waves use (docs/waves.md):

| Plane | Carries | Physics | Today |
|---|---|---|---|
| **Control** — chain rows | commands, keystrokes, requests, answers, low-rate readings | ordered, ACKed, hash-linked, ephemeral | shell, Stop, reset, pigeon announce |
| **Bulk** — PT streams over spools | files in both directions, trees, screenshots | chunked, resumable, spooled on disk, windowed per peer | pigeon chunks |
| **Live** — ephemeral UDP media | screen, camera, mic, high-rate sensors, input | lossy, latest wins, never ratcheted per packet, stepped keys | the wave engine (voice) |

**Wire shape.** A verb is a typed field in `BridgeWire`, not a new `RefKind` each time. A new `RefKind` appears only where a row's semantics truly differ. Old parsers drop unknown fields. An unknown or disabled verb gets a typed "not offered here" answer, so the operator never waits on silence.

**Offer.** The host answers a `BridgeReset` with a typed **offer**: its platform plus the verbs it has switched on. The operator's session shows buttons only for those verbs. This is an edge: it is sent again when a host switch changes, and never polled.

### Files, both ways

- **Homing pigeons** — ask for a path on the host (`take <path>`, or a picker over the host's tree), and the file flies back into the operator's download folder, or into a folder you choose. It uses the same spool, hash, progress bar and landing law, pointed the other way. The request names a path because the operator is asking, and the host resolves that path against its own shell's cwd.
- **Trees** — dropping a folder sends a manifest of relative paths plus one pigeon per file, and the tree lands under its own name. A relative path containing `..`, or pointing through a symlink out of the landing root, is refused at the host.
- **Resume after restart on both sides** — this is stage 4 of the PT consolidation (`ReceiveBuffer` backed by a spool, docs/PT.md). Attachments move onto the same spool afterwards, so there is one story for bulk transfer.
- **Clipboard** — push the operator's clipboard to the host, or pull the host's back, as typed text or image. There is no automatic sync: it is a gesture each time, and nothing crosses unasked.

### Interactive terminal (PTY)

Today a program that waits for input (vim, top, a REPL, a sudo prompt) just waits until you press Stop. The PTY tier gives such a program a real terminal:

- The host allocates a PTY (`forkpty`) for the command, **as well as** keeping the line shell. The line shell stays the default because its transcript can be read, copied and searched.
- Keystrokes go operator → host as typed key events on the control plane, ordered and ACKed. A dropped keystroke is unacceptable.
- The screen travels as **state, not a byte stream**: the host keeps the cell grid and sends damage (changed cells since the last acked state) on the live plane. Loss costs nothing, because the next frame carries the current truth. That is the mosh insight, and it is also the lesson of the 2026-07 term-frame model: that model fire-and-forgot a byte stream, never delivered, and was excised.
- Fluor renders the grid with the bundled monospace faces, and the operator's own keyboard (or the phone's IME) supplies the keys.

### Screen and input

The RustDesk replacement. The screen is a **beam** (video, docs/lexicon.md) of the host's display.

- **Capture** per host platform: PipeWire through the ScreenCast portal on Wayland (restore token remembered), XShm/XComposite on X11, ScreenCaptureKit on macOS, Desktop Duplication on Windows, and MediaProjection on Android (the OS demands its own consent prompt; law 2 applies as far as the OS allows).
- **Encoding** follows the beam design once beams are built. Screens differ from cameras: large static regions, sharp text and bursty damage, so the encoder should favour damage rectangles and lossless text over smooth motion.
- **Input** goes operator → host on the control plane (ordered, reliable): pointer, buttons, scroll, keys, text. Injection per platform: uinput on Linux (below X and Wayland alike), CGEvent on macOS, SendInput on Windows, an AccessibilityService on Android. **Avoid XTEST.** The 2026-09 three-day outage started with RustDesk's XTEST cursor racing an HDMI hotplug inside the amdgpu DDX and killing Xorg, which killed the bridge with it (docs/headless-lifeline.md).
- **View-only is its own verb**, separate from **control**. A host can offer watching without offering driving.
- **Displays before login.** A host at a greeter or locked screen should still be viewable and drivable by its own members. That means capture and injection run from the lifeline process, not from the UI session.

### Sensors: the phone as instrument

A phone is a box of sensors your desktop does not have. The bridge makes them a member's instruments:

- **Mic** — a *listen*: a one-way wave from the host's mic, carried by the existing wave engine and keys, with no ringing on the host. Uses: hearing the room the phone was left in, or using the phone as the desktop's mic.
- **Camera** — a one-way beam from a chosen camera. Uses: the phone as the desktop's webcam, fed into a virtual camera device (v4l2loopback on Linux, a camera extension on macOS), or checking on the room the phone is in.
- **Location** — a fix: latitude and longitude, accuracy, altitude, fix time. Requested once, or subscribed to on the platform's own movement callback (by distance, never by our timer). Low rate, so it rides the control plane.
- **Motion** — accelerometer, gyroscope, magnetometer, barometer, at the sensor's own rate. High rate, so it rides the live plane. Uses: the phone as an air mouse or a 3D controller for the desktop, a vibration probe held against a machine, a level, a seismograph.
- **Device state** — battery and charging, temperatures, storage, network (Wi-Fi, carrier, which path the bridge is on), screen on or off. Low-rate, event-edged readings.
- **Recording** — any stream can be written on the host to a file with typed units (VSF, numbers binary, never CSV). When you stop it, the file flies home as a pigeon. A GPS track of a walk, an accelerometer trace of a drive, an hour of room audio.

Readings are typed VSF fields with units, the same as everything else at rest. Nothing is formatted into text on the host.

### Find and signal

The passless "find my device", with no Apple or Google account between you and your own phone:

- **Ring** — loud, at full volume, even on silent or do-not-disturb, until you touch the phone or press stop on the operator.
- **Locate** — one location fix, answered on the host's next network edge.
- **Show** — put a message on the host's screen ("this phone belongs to …, please wave …"), using a petname or your chosen words, never your handle.
- **Flash and buzz** — the torch and the vibration motor, for finding it under the couch.

These are the one family where law 1 needs a decision (below): a stolen phone is exactly the one you want to find.

### Reach

- **Tunnels** — forward a TCP port across the bridge: a local listener on the operator, connections carried on the bulk plane, and the host connecting to a destination it allows. Uses: reaching a dev server on your desktop from your phone, a NAS admin page, a printer on the home LAN, all without a VPN or an open port. Destinations are part of the host's switch (law 3).
- **Wake** — ask an awake member on the same LAN as a sleeping one to send it a Wake-on-LAN packet. Then the lifeline brings the bridge up.
- **Fan-out** — run one command on several members at once, with one output row per host, streamed and stopped independently. Uses: `git pull` everywhere, "what version are you all on".
- **Scripting** — `photon bridge <device petname> -- <cmd>` from a terminal on the operator, driving the running local instance through its control channel. Output goes to stdout and the host's exit code becomes the local exit code. Deploys and cross-machine scripts use the bridge the way they use `ssh` today, with no keys to manage.

### Hosts everywhere

| Host | Shell | Files | Screen | Sensors | Notes |
|---|---|---|---|---|---|
| Linux | bash (shipped) | ✅ pigeons | portal / X11 | — | lifeline shipped |
| macOS | bash (shipped) | ✅ pigeons | ScreenCaptureKit | — | TCC grants once |
| Windows | PowerShell via ConPTY | planned | Desktop Duplication | — | needs a lifeline service |
| Android | none (app sandbox only) | app storage + SAF | MediaProjection | ✅ the point | foreground service for mic, camera and location |
| Redox / ferros | later | later | later | — | |

An Android host offers **verbs, not a shell**: its value is its sensors, its files and find. A shell inside the app sandbox would be a toy that looks like a promise.

## Open decisions

- **Finding a locked-out device.** Lockout is the answer to theft (a stolen device is locked out, never expelled). A locked-out device refuses to act as the identity. Should it still answer **ring and locate** from members, and only those, so a stolen phone can be found? And should its indicator (law 4) stay visible to whoever is holding it? Recommendation: yes, it answers only those two verbs, and the indicator stays. A thief who learns they are being located has learned nothing that helps them, and a device that locates silently is a device that could be made to spy.
- **The shell's default.** Today every desktop member's shell is open to every other member. That is consistent with passless ownership, but it makes each member as powerful as the most exposed one. Keep the shell on by default for desktops and off for phones, behind the same host switch as everything else.
- **Leases.** A leased device runs the guest's identity as a proxy, so neither the owner nor the guest can bridge it (law 1). Whether the owner should keep **find** over leased hardware (the title never moves) is a question for docs/device-lease.md, not for the bridge.
