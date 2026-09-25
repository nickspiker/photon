# AGENT.md audit — 2026-09-24

Scope: the whole photon tree (`src/`, the Android Kotlin), swept for AGENT.md's format rules — text carrying structure, decimal encodings, manual binary layouts, bare VSF sections, floats on instants, and lingering compatibility arms.
Not in scope this pass: Rule 0 (bounds and saturating arithmetic), the Clamp Trap, GUI continuity and power-of-two constants — those want a read of the arithmetic, not a format sweep.
Method: targeted greps for each pattern, then every hit read in context; hash transcripts and signing inputs (`to_le_bytes` into a hasher) are not serialization and are excluded.

## Fixed today

- The five content-prefix encodings (`photon-wave`, `-era`, `-delete`, `-molecule`, `-attach`) and the probe sentinel — a row's kind is now a typed field on every carrier (`types/row_control.rs`, commit 769924f0).
- The era rows' decimal `era_next` / hex `prior_tag`, the delete marker's decimal target, the attachment's decimal size — all typed.
- The attachment filename used as a type tag (`"wave.audio"`, `"wave.env"`) — `AttachRole`.
- The express wave frame's `[ts][flag][key][content utf8]` payload — a complete VSF document.
- `eagle_time_now`'s f64 product (vsf 0bfde19) and `boot_osc`'s f64 conversions (a425dd7a).

## HIGH — structure smuggled through text, or decimal indexing

**H1. The server's clock scraped out of an error string.** The FGTW worker writes `server_now=<decimal oscillations>` into a refusal's human detail text, and `network/time_base.rs:119` `server_now_from_detail` finds the substring and parses the digits. A client/server contract living inside prose: reword the message and clock recovery silently dies. Fix: a typed `server_now` (e6) field on the refusal document; the worker and the client change together.

**H2. Profile fields are decimal-indexed.** Repeated profile entries are stored as `profile.{base}{n}` — `phone2`, `phone3`… (`ui/photon_app/settings.rs:1225`, `:1232`) — and `ui/photon_app/protocol.rs:1223` recovers the index with `field_id[base.len()..].parse::<u32>()`. This is the exact pattern AGENT.md's "Decimal/Arabic Indexing is FORBIDDEN" section shows as WRONG. Fix: one multi-value field per base, the entry order is the index.

**H3. Fleet settings keys carry ids as text.** The settings store is keyed by dotted strings, and many keys embed an identifier: `fleet.name.<hex device key>` (`settings.rs:629`, `photon_app.rs:3861`), `fleet.locked.<hex>` / `fleet.released.<hex>` (`devices.rs:331`, `:1882`, `:1900`, `driver.rs:1152`), `fleet.unlockack.<hex>.<hex>` (`devices.rs:1959-1987`, found again by prefix), `react.recent.<hex8>` (`settings.rs:1027`), `share.<field id>` (`protocol.rs:558`), `audio.cal.echo.<route>.{g,delay}` and `audio.cal.voice.<mic>.{voiced,floor}` (`settings.rs:724-876`), and `settings.rs:854` splits the mic name back out with `rsplit_once('.')`; `storage/fleet_settings.rs:280` splits keys to recover their family. A device key is binary; as hex in a key it is un-typed, doubled, and unqueryable except by string surgery. Fix: the key names the KIND (`fleet.name`), the entry carries the subject as a typed field (`ke` device, route/mic as their own fields); lookups match on the field.

**H4. The bridge still runs untyped rows as commands.** `ui/photon_app/conversation.rs:1684` — a bare sibling row with no `RefKind::BridgeCmd` is stripped of a `$ ` content prefix and EXECUTED, "for ONE transition era… delete this arm once the fleet is past the 2026-08-23 line". A month past, still live: content-as-trigger, the class the typed reference was built to kill, and a remote-execution path keyed on text. Fix: delete the arm.

**H5. Three vault records are bare sections, not complete VSF files.** `storage/contacts.rs:88` (the contact list), `:373` (per-contact state) and `:633` (the sibling index) write `builder.encode()` straight to the vault — no header, provenance hash or TOC, against "ALL network transport and disk storage MUST use complete VSF files". Every other store here wraps with `VsfBuilder`. Fix: wrap, read with `parse_document`, and let the loaders refuse the bare form (flag day, like the pages were).

## MEDIUM — custom binary at rest or on the wire

**M1. `BlobManifest`** (`storage/mod.rs:340`): `[size u64 LE][chunk_size u32 LE][n u32 LE][n × 32]`, "binary at rest and on the wire" — every chunked blob's table of contents, outside VSF. Fix: a manifest section (size, chunk size, a multi-value `hb` per chunk).

**M2. Checkpoint custody state** (`network/fgtw/fleet.rs:398` `ckpt_state_bytes`): `k ‖ epoch ‖ prev_k ‖ prev`, zero-filled when absent, served and held in custody. Fix: a section with an optional `prev`.

**M3. Marks at rest are a private codec.** The durable record stores `encode_marks` (`types/contact.rs:160`: `[kind u8][start u32][len u32][dlen u16][dest]…`) inside one Bytes field — while the live package and the history page already carry marks as typed multi-value fields. Fix: the same four typed fields in the record.

**M4. FIXED for history pages (2026-09-25; the live package is unchanged). Page byte-columns are one `u` per byte.** `network/history_pages.rs:201` and `:216` write the envelope thumbnail and micro preview as a multi-value of single-byte unsigned values — a 1728-byte preview becomes 1728 typed values. The value is opaque bytes, not a list of numbers. Fix: one opaque-bytes value per row.

**M5. Socket addresses packed by hand** (`network/fgtw/protocol.rs:212`): raw octets plus a big-endian port, with the IP version inferred from the length on read (`:236`, `:243`). VSF has network types (`ns` socket). Fix: use them.

**M6. The wave stack has more non-VSF formats than it admits.** `wave/packet.rs:1` calls the media datagram "the ONE non-VSF datagram in the system" — an owner-sanctioned exception for a 5-byte header at 100 packets/second. But the fill plane is a second non-VSF datagram (magic 0xC8, the hand codec `FillMsg` at `wave/engine.rs:1515`), the spool records are a third format (`wave/spool.rs`: `[len u16][sealed [chan][osc][seq][slot][gain][verdict][frame]]`), and the kept recording is a custom binary container at rest (`wave/record.rs` PHWAVE9 header and slot layout) — the one Rule -1 covers most squarely, since it is a stored file. Fix: either write the fill plane into the sanctioned exception with its reason, or give it a VSF frame; replace PHWAVE9 with the LOCK at-rest pages (docs/waves.md "The canonical wave", this plan's Part C), which are VSF by design.

**M7. The transfer spool file** (`storage/spool.rs:125`): an 8-byte LE length, then a VSF prelude document, then a raw sparse body. The prelude is proper; the framing around it is not.

## LOW

- **L1.** Audio route ids are `kind:name` strings split at render (`ui/photon_app/render.rs:871`).
- **L2.** The Wi-Fi Direct JNI edge passes IPv4 addresses as strings to be re-parsed (`platform/jni_android.rs:1733`).
- **L3.** An absolute capture stamp goes through f64 in the v-chirp lag estimate (`wave/vchirp.rs:282`, `cap_anchor_osc as f64` — ~256-oscillation grain at today's counts); RTTs via `as_secs_f64` in `fgtw/bootstrap.rs:78` and `fgtw/blob.rs:340` are durations and harmless.
- **L4.** DNS-SD TXT chunks are keyed `t0`, `t1`… (`PhotonWifiDirect.kt:169`). The TXT record is a foreign text format with a 255-byte value cap, so chunking is forced; the decimal key names are the only choice DNS-SD leaves. Recorded, not a fix.

## Suggested order

H4 first (a remote-execution path keyed on text, one deletion). Then H1 (clock recovery hangs on a string), H5 (a flag day on three records), H2 and H3 together (both are the settings store's key shape), then M3/M4 with the next page-format change, M1/M2/M5 as their subsystems are next touched, and M6 inside the LOCK container work.
