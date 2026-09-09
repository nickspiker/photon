# Release notes

Shipped versions are named by their minor number alone (v87); patch numbers belong to dev builds and never appear here.
The top section is always `## Upcoming` and collects what has landed on main since the last release. A deploy renames it to the version it ships as and opens a fresh `## Upcoming` above it.
The app shows the section for the version it runs, and the photon page on holdmyoscilloscope.com shows the newest few.
One sentence per line, however long; plain language for the person who installs it, not for the person who built it.

## Upcoming

- A web address turns into a link while you type it, not only after you send.
- On desktop, pasting a web address onto selected text turns that text into a link.
- Alerts at the bottom of the contact and attest screens now stack, and the update notice stays until the update is on.
- The update notice only appears while automatic checking is enabled.
- Settings rows wrap at the pane edge under a large font instead of running off it.

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
