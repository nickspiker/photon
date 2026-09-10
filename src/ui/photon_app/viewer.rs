//! The attachment VIEWER and READER (typed attachments Phase 2/3, 2026-09-10): full-pane overlays inside the conversation — an image viewer (fit / zoom about the pointer / pan / previous-next image / Original / Save) and a text reader (unwrapped lines, vertical + horizontal scroll). Both are conversation-scoped state, closed by Back, Escape, or the Android back gesture; neither is a timer.

use super::*;

/// The open image: which row, which pixels are available (the "Original" decode outranks the preview blob outranks the row's micro thumb), and the view transform.
pub(super) struct Viewer {
    pub ci: usize,
    pub hash: [u8; 32],
    pub preview_hash: Option<[u8; 32]>,
    pub name: String,
    pub kind: crate::types::AttachKind,
    /// Zoom relative to fit-to-pane (1.0 = fit).
    pub zoom: f32,
    /// Pan in pane pixels from the centred position.
    pub pan: (f32, f32),
    /// The Original decode was requested (its pixels land in the image cache under the content hash).
    pub full_requested: bool,
}

/// The open text file: its lines and the scroll position.
pub(super) struct Reader {
    pub hash: [u8; 32],
    pub name: String,
    pub lines: Vec<String>,
    pub scroll: f32,
    pub hscroll: f32,
}

/// Largest text file the reader will open (a bigger one saves instead).
const READER_MAX_BYTES: usize = 4 * 1024 * 1024;

/// The decoded-picture cache type (hash → (w, h, packed pixels); None = decode failed).
pub(super) type ImgCache = std::collections::HashMap<[u8; 32], Option<(usize, usize, Vec<u32>)>>;

/// Lines an image row's picture band reserves: the decoded preview blob's tall band, the micro thumb's short one, or none. A free function over the cache so the render (which holds the chrome borrow) can call it.
pub(super) fn img_band_lines_of(cache: &ImgCache, m: &crate::types::ChatMessage) -> usize {
    let Some(a) = m.attach else {
        return 0;
    };
    if !a.kind.is_image() {
        return 0;
    }
    if a.preview_hash.is_some_and(|ph| matches!(cache.get(&ph), Some(Some(_)))) {
        return super::render::IMG_PREVIEW_LINES_FULL;
    }
    if crate::types::parse_micro_image(&m.preview).is_some() {
        return super::render::IMG_PREVIEW_LINES;
    }
    0
}

/// The image the viewer shows right now: (w, h, pixels) — the Original decode, else the preview blob, else the row's micro thumb (found in `msgs`, the open conversation's rows).
pub(super) fn viewer_pixels_of<'a>(v: &Viewer, cache: &'a ImgCache, msgs: &[crate::types::ChatMessage]) -> Option<(usize, usize, std::borrow::Cow<'a, Vec<u32>>)> {
    if let Some(Some((w, h, px))) = cache.get(&v.hash) {
        return Some((*w, *h, std::borrow::Cow::Borrowed(px)));
    }
    if let Some(Some((w, h, px))) = v.preview_hash.and_then(|ph| cache.get(&ph)) {
        return Some((*w, *h, std::borrow::Cow::Borrowed(px)));
    }
    let micro = msgs.iter().find_map(|m| {
        let (h, _, _) = crate::types::parse_attachment_content(&m.content)?;
        (h == v.hash).then(|| crate::types::parse_micro_image(&m.preview).map(|(w, h, px)| (w, h, crate::ui::attach_preview::micro_to_display(px))))
    })??;
    Some((micro.0, micro.1, std::borrow::Cow::Owned(micro.2)))
}

impl PhotonApp {
    /// Open the viewer on an image row (the preview blob or micro thumb shows at once; the Original decode is a pill away).
    pub(super) fn open_viewer(&mut self, ci: usize, hash: [u8; 32]) {
        let Some((meta, name)) = self.conv_of(ci).and_then(|v| {
            v.messages.iter().find_map(|m| {
                let (h, n, _) = crate::types::parse_attachment_content(&m.content)?;
                (h == hash).then_some((m.attach?, n))
            })
        }) else {
            return;
        };
        self.viewer = Some(Viewer {
            ci,
            hash,
            preview_hash: meta.preview_hash,
            name,
            kind: meta.kind,
            zoom: 1.0,
            pan: (0.0, 0.0),
            full_requested: false,
        });
        self.reader = None;
        self.selected_msg = None;
        self.scene_dirty = true;
        crate::log("attach: viewer opened");
    }

    /// Decode the ORIGINAL bytes for the open viewer, off-thread (a RAW goes thru a temp file for limbus; the temp lives in the runtime dir and is removed when the decode lands).
    pub(super) fn request_full_image(&mut self) {
        let Some(v) = self.viewer.as_mut() else {
            return;
        };
        if v.full_requested || self.img_cache.contains_key(&v.hash) || self.img_pending.contains(&v.hash) {
            return;
        }
        v.full_requested = true;
        let (hash, name, kind) = (v.hash, v.name.clone(), v.kind);
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        self.img_pending.insert(hash);
        let tx = self.img_decoded_tx.clone();
        queue_job(&self.seal_job_tx, move || {
            let Some(bytes) = crate::storage::blob_load(&seed, &hash) else {
                let _ = tx.send((hash, None));
                return;
            };
            let raw_tmp = super::attachments::raw_temp_path(kind, &hash, &bytes);
            let out = crate::ui::attach_preview::full_image(&bytes, &name, kind, raw_tmp.as_deref());
            if let Some(p) = raw_tmp {
                let _ = std::fs::remove_file(p);
            }
            crate::logf!("attach: original decoded {}", out.as_ref().map_or("(failed)".to_string(), |(w, h, _)| format!("{w}×{h}")));
            let _ = tx.send((hash, out));
        });
        self.scene_dirty = true;
    }

    /// Step the viewer to the previous (−1) or next (+1) image row in the conversation.
    pub(super) fn viewer_step(&mut self, dir: i32) {
        let Some(v) = self.viewer.as_ref() else {
            return;
        };
        let (ci, cur) = (v.ci, v.hash);
        let images: Vec<[u8; 32]> = self
            .conv_of(ci)
            .map(|c| {
                c.messages
                    .iter()
                    .filter(|m| !m.deleted && m.attach.is_some_and(|a| a.kind.is_image()))
                    .filter_map(|m| crate::types::parse_attachment_content(&m.content).map(|(h, _, _)| h))
                    .filter(|h| crate::storage::blob_present(h) || self.img_cache.contains_key(h))
                    .collect()
            })
            .unwrap_or_default();
        let Some(i) = images.iter().position(|h| *h == cur) else {
            return;
        };
        let j = i as i32 + dir;
        if j < 0 || j as usize >= images.len() {
            return;
        }
        self.open_viewer(ci, images[j as usize]);
    }

    /// Open the reader on a text/code row (the blob must be held).
    pub(super) fn open_reader(&mut self, hash: [u8; 32], name: String) {
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let Some(bytes) = crate::storage::blob_load(&seed, &hash) else {
            return;
        };
        if bytes.len() > READER_MAX_BYTES {
            self.ready_toast = Some(tr(Msg::ReaderTooLarge).into_owned());
            self.ready_toast_screen = None;
            return;
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<String> = text.lines().map(|l| l.replace('\t', "    ")).collect();
        self.reader = Some(Reader { hash, name, lines, scroll: 0.0, hscroll: 0.0 });
        self.viewer = None;
        self.selected_msg = None;
        self.scene_dirty = true;
        crate::log("attach: reader opened");
    }

    /// Does any picture exist for this attachment row (decoded preview, or a micro thumb on the row) — the viewer can open without the original.
    pub(super) fn img_wants_any_picture(&self, ci: usize, hash: &[u8; 32]) -> bool {
        self.conv_of(ci).is_some_and(|c| {
            c.messages.iter().any(|m| {
                crate::types::parse_attachment_content(&m.content).is_some_and(|(h, _, _)| h == *hash)
                    && (crate::types::parse_micro_image(&m.preview).is_some()
                        || m.attach.and_then(|a| a.preview_hash).is_some_and(|ph| matches!(self.img_cache.get(&ph), Some(Some(_)))))
            })
        })
    }

    /// Close whichever overlay is open. Returns true when one was.
    pub(super) fn close_viewers(&mut self) -> bool {
        let was = self.viewer.is_some() || self.reader.is_some();
        self.viewer = None;
        self.reader = None;
        if was {
            self.scene_dirty = true;
        }
        was
    }


    /// Decoded previews the worker finished: into the cache (a failure is remembered as None so the walk stops asking), the wrap re-measures (the band grows to the preview size).
    pub(super) fn drain_img_decoded(&mut self) {
        while let Ok((hash, px)) = self.img_decoded_rx.try_recv() {
            self.img_pending.remove(&hash);
            self.img_cache.insert(hash, px);
            self.msg_wrap = None;
            self.scene_dirty = true;
        }
    }

    /// Preview wants the last render collected: a held preview blob → decode job; a missing one → one fetch per session (the friend + every sibling answer).
    pub(super) fn drain_img_wants(&mut self) {
        let wants = std::mem::take(&mut self.img_wants);
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        for (peer, hash, held) in wants {
            if self.img_cache.contains_key(&hash) || self.img_pending.contains(&hash) {
                continue;
            }
            if held {
                self.img_pending.insert(hash);
                let tx = self.img_decoded_tx.clone();
                queue_job(&self.seal_job_tx, move || {
                    let out = crate::storage::blob_load(&seed, &hash).and_then(|b| crate::ui::attach_preview::decode_preview_blob(&b));
                    let _ = tx.send((hash, out));
                });
            } else if self.attach_auto_fetched.insert(hash) {
                if let Some(ci) = self.contacts.iter().position(|c| c.handle_hash == peer) {
                    self.attach_fetch(ci, &hash);
                }
            }
        }
    }
}
