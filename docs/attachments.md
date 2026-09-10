# Attachments — typed kinds, two preview tiers, chunked transport, viewers

Built 2026-09-10 (Nick's decisions that day: house-only video, two preview tiers, images incl. RAW and code/text first, chunked transport now).
The doctrine every part obeys: an original is never rewritten — the bytes that travel are the bytes that were picked and the row's hash is their hash; photon encodes only its own house formats (AV1-in-VSF previews, never JPEG/PNG — `scripts/lib/artifact-gate.sh` enforces); no draw path ever decodes an original — previews are computed once, off the UI thread, at send or at install.

## The row

An attachment row keeps its identity in the content string (`ATTACHMENT_PREFIX ‖ blake3 ‖ name ‖ size`, types/contact.rs) and carries TYPED extras beside it (types/attach_kind.rs):

- `attach: Option<AttachMeta { kind, dims, preview_hash }>` — the kind is SNIFFED from magic bytes at send (`sniff`), never trusted from a name or the wire; the receiver re-sniffs the installed bytes and keeps the stricter verdict (`AttachKind::reconcile`: a program dressed as a picture reads as a program).
- `preview: Vec<u8>` — the MICRO tier: a ≤24-px gamma-2 VSF RGB thumb (`[w][h][rgb…]`, ≤ 1730 bytes) for images, the first 240 bytes for text/code. It rides the row itself, so every device draws it before any blob exists.

Carriage: vault fields (`attach_kind/w/h/ph/pv`), fleet page columns (`m_ak/aw/ah/ahn/aph/apn/apv` — remember `page_schema()`), and typed package fields on the friend wire (`ak/aw/ah/aph/apv`).

Kinds: Unknown, Image, RawImage (camera RAW/DNG/VSF spectral), Video, Audio, Text, Code, Archive, Program, Document. The bubble glyph and the tap follow the kind: images open the viewer, text/code the reader, everything else saves. Programs never run.

## The preview blob

`ui/attach_preview.rs`: ONE linear decode per picked image feeds both tiers — legacy formats thru the `image` crate (EXIF orientation, sRGB assumed: the honest `assumed` tier), JPEG XL thru jxl-oxide from its tagged primaries and transfer, camera RAW / DNG thru limbus (native-depth counts, 2×2 Bayer bin with no demosaic, the DNG matrix to XYZ, auto-white on the brightest channel — the picture lumis embeds). The PREVIEW tier is that decode at ≤512 px, AV1 (`avatar::encode_av1_wh`, q 40) in the avatar's VSF image container (`vsf::builders::compressed_image`), content-addressed, stored under its own hash, named by the row (`preview_hash`), and pushed to the friend AHEAD of the original. A device that holds a row but not its preview blob asks for it once per session (`drain_img_wants` → `attach_fetch`); decoded pictures live in `img_cache` for the card and the viewer.

## Transport (chunked, resumable)

`storage/mod.rs`: a blob past `BLOB_CHUNK_SIZE` (256 KB) is stored as content-addressed CHUNKS (each a vault value at its own chunk-hash address) plus a MANIFEST (`BlobManifest`, size + chunk size + chunk hashes) at a keyed address under the whole-file hash. `blob_present` is one vault probe for a whole-value blob and a proven-complete set lookup for a chunked one; `blob_write_file` streams chunks to disk with a running hash (Save never rebuilds the file in RAM); `blob_delete` sheds chunks and manifest.

Wire (`network/fgtw/protocol.rs`): `attach_manifest` (sealed manifest) then `attach_chunk` (index + sealed bytes), both under the same relationship key as `attach_blob`; `attach_req` carries an optional WANT bitmap — a resuming fetch that already holds the manifest asks only for the chunks it lacks, and a serve answers only those. Chunks verify against the manifest off-thread; the last one fires `attach_have`. Progress is keyed by hash (`attach_chunk_progress`). Cap: 256 MB (the picker still hands bytes over whole — RAM at pick time is the bound).

## Viewer and reader (`ui/photon_app/viewer.rs`)

Overlays inside the conversation (painted before the row walk, hit-stamped after it): the viewer shows the Original decode, else the preview blob, else the micro thumb; fit-to-pane, wheel/pinch zoom about the pointer, drag pan, ←/→ between the conversation's images, Back / Original (decode the file at ≤4096 px, off-thread; a RAW thru a temp copy for limbus) / Save. The reader shows a text/code file unwrapped with vertical and horizontal scroll (≤ 4 MB; bigger saves).

## Later

- "Send as VSF" for RAW (limbus/opsin `write_vsf` translateration → a second attachment).
- Foreign audio decode-only ingest behind a `KeptStream`-shaped source so the wave card plays audio files.
- A VSF video container (AV1 frames with eagle stamps + Opus track); foreign video stays byte-exact with a film pill.
- Streaming picker read on Android (a content URI read in chunks), eager sibling replication of originals, scoped-blobs for attachments.
