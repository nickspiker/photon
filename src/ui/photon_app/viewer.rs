//! The attachment VIEWER and READER (typed attachments Phase 2/3, 2026-09-10): full-pane overlays inside the conversation — the image viewer and a text reader (unwrapped lines, vertical + horizontal scroll). Both are conversation-scoped state, closed by Back, Escape, or the Android back gesture; neither is a timer.
//!
//! THE IMAGE VIEWER IS OPSIN'S (Nick 2026-09-17: "implement the same interface opsin has in Photon for the image viewer, DNG and all, exposure slider, etc. … not keen on the current state of it being weird and hand-rolled"): once the original decodes, [`opsin::view::View`] — the image area, the tool panel (navigator, 1:1/Fit/Save/Info, CCW/CW/Crop, the histogram with its HDR/X/Y/Clip pills, the exposure slider, the chromaticity chart), the frame-info HUD and every key binding — is hosted whole inside the conversation screen, with photon's Back pill at the top-left and Escape / the back gesture to leave. Until the decode lands the row's preview blob or micro thumb shows fitted, with the name and the state; a picture not held here is fetched once.

use super::*;

/// The open image: which row, and opsin's view once the original is decoded.
pub(super) struct Viewer {
    /// The conversation's contact, by id: the viewer stays open across ticks, and an index would drift onto another contact after a removal.
    pub contact: ContactId,
    pub hash: [u8; 32],
    pub preview_hash: Option<[u8; 32]>,
    pub name: String,
    pub kind: crate::types::AttachKind,
    /// opsin's viewer — the whole interface — once the decode lands. Its widgets carry the ids at `viewer_view_base`, so hover, press and the overlay tables ride photon's Container walk.
    pub view: Option<opsin::view::View>,
    /// The decode job is out (off the UI thread; `drain_img_view` installs the result).
    pub decoding: bool,
    /// Nothing could open these bytes (opsin and the image crate both declined) — the preview stays, the log says why.
    pub failed: bool,
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

/// Hit ids reserved for the view's widgets (a slider and twelve pills today; room for the opsin features photon never builds).
pub(super) const VIEW_HIT_IDS: HitId = 16;

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

/// Lines an AUDIO row's waveform band reserves — a pigeon carrying a wave (Nick 2026-09-12): the band derives from the audio itself, so it exists once the blob is held. Wave recordings fold into their wave card instead.
pub(super) fn audio_band_lines_of(m: &crate::types::ChatMessage) -> usize {
    let Some(a) = m.attach else {
        return 0;
    };
    if a.kind != crate::types::AttachKind::Audio || m.is_wave_recording() {
        return 0;
    }
    let Some((h, _, _)) = m.file_parts() else {
        return 0;
    };
    if crate::storage::blob_present_or_pending(&h) { super::render::IMG_PREVIEW_LINES } else { 0 }
}

/// The picture shown while the original decodes: (w, h, pixels) — the preview blob, else the row's micro thumb (found in `msgs`, the open conversation's rows).
pub(super) fn viewer_pixels_of<'a>(v: &Viewer, cache: &'a ImgCache, msgs: &[crate::types::ChatMessage]) -> Option<(usize, usize, std::borrow::Cow<'a, Vec<u32>>)> {
    if let Some(Some((w, h, px))) = v.preview_hash.and_then(|ph| cache.get(&ph)) {
        return Some((*w, *h, std::borrow::Cow::Borrowed(px)));
    }
    let micro = msgs.iter().find_map(|m| {
        let (h, _, _) = m.file_parts()?;
        (h == v.hash).then(|| crate::types::parse_micro_image(&m.preview).map(|(w, h, px)| (w, h, crate::ui::attach_preview::micro_to_display(px))))
    })??;
    Some((micro.0, micro.1, std::borrow::Cow::Owned(micro.2)))
}

impl PhotonApp {
    /// Open the viewer on an image row: the preview shows at once, the original's decode starts (or its fetch, when it is not held here).
    pub(super) fn open_viewer(&mut self, ci: usize, hash: [u8; 32]) {
        let Some((meta, name)) = self.conv_of(ci).and_then(|v| {
            v.messages.iter().find_map(|m| {
                let (h, n, _) = m.file_parts()?;
                (h == hash).then_some((m.attach?, n))
            })
        }) else {
            return;
        };
        let Some(contact) = self.cid(ci) else { return };
        self.viewer = Some(Viewer { contact, hash, preview_hash: meta.preview_hash, name, kind: meta.kind, view: None, decoding: false, failed: false });
        self.reader = None;
        self.selected_msg = None;
        self.scene_dirty = true;
        crate::log("attach: viewer opened");
        self.request_view_decode();
    }

    /// Decode the ORIGINAL for the open viewer, off-thread: opsin's ingest for everything it decodes (DNG/RAW/TIFF/JXL/JPEG/WebP/VSF — thru a temp file in the runtime dir, its readers want a path), the image crate's linear sRGB decode handed to opsin as linear VSF RGB for the rest (PNG, GIF, BMP), so it is ONE viewer whatever the bytes. The display copy folds to [`crate::ui::attach_preview::LINEAR_VIEW_MAX_EDGE`] (twelve bytes a pixel); a phone drops the decode itself once its facts are captured. A picture not held here is fetched once; the tick re-runs this when it lands.
    pub(super) fn request_view_decode(&mut self) {
        let Some(v) = self.viewer.as_mut() else {
            return;
        };
        if v.view.is_some() || v.decoding || v.failed {
            return;
        }
        let (contact, hash, name, kind) = (v.contact, v.hash, v.name.clone(), v.kind);
        match crate::storage::blob_present_known(&hash) {
            Some(true) => {}
            Some(false) => {
                if let Some(ci) = self.ci_of(&contact) {
                    if self.attach_auto_fetched.insert(hash) {
                        self.attach_fetch(ci, &hash);
                    }
                }
                return;
            }
            // The probe is on its way; the tick asks again.
            None => return,
        }
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        v.decoding = true;
        let tx = self.img_view_tx.clone();
        queue_job(&self.seal_job_tx, move || {
            let out: Result<opsin::view::Loaded, String> = (|| {
                let bytes = crate::storage::blob_load(&seed, &hash).ok_or_else(|| "the blob would not open".to_string())?;
                let keep_decode = !cfg!(target_os = "android");
                let edge = Some(crate::ui::attach_preview::LINEAR_VIEW_MAX_EDGE);
                if !matches!(opsin::sniff::sniff(&bytes), opsin::sniff::Kind::Unknown) {
                    let path = super::attachments::view_temp_path(&name, &hash, &bytes).ok_or_else(|| "no runtime dir for the decode".to_string())?;
                    let r = opsin::view::load_image_folded(&path, edge, keep_decode);
                    let _ = std::fs::remove_file(&path);
                    match r {
                        Ok(loaded) => return Ok(loaded),
                        Err(e) => crate::logf!("attach: opsin declined {}: {} — image-crate path", if name.is_empty() { "the bytes" } else { name.as_str() }, e),
                    }
                }
                let (w, h, planar) = crate::ui::attach_preview::legacy_linear_planar(&bytes, &name, kind, crate::ui::attach_preview::LINEAR_VIEW_MAX_EDGE).ok_or_else(|| "no decoder opened these bytes".to_string())?;
                let dec = opsin::convert::ingest_linear_vsf_rgb(w, h, planar, "assumed_srgb (image crate)");
                opsin::view::Loaded::from_decoded(dec, None, &name, bytes.len() as u64, None, keep_decode)
            })();
            match &out {
                Ok(l) => crate::logf!("attach: original decoded for the viewer — {}×{}", l.dims().0, l.dims().1),
                Err(e) => crate::logf!("attach: viewer decode failed: {}", e),
            }
            let _ = tx.send((hash, out));
        });
        self.scene_dirty = true;
    }

    /// A decode landed: build opsin's view around it (widget ids from the reserved block, the export pill reading Save — photon hands the original over, no JPEG is written). A blob that arrived after the viewer opened (an auto-fetch) starts its decode here too.
    pub(super) fn drain_img_view(&mut self) {
        while let Ok((hash, out)) = self.img_view_rx.try_recv() {
            let base = self.viewer_view_base;
            let Some(v) = self.viewer.as_mut().filter(|v| v.hash == hash) else {
                continue;
            };
            v.decoding = false;
            match out {
                Ok(loaded) => {
                    let mut counter = base;
                    let mut view = opsin::view::View::new(loaded, &mut counter);
                    view.set_export_label(&tr(Msg::SavePill));
                    v.view = Some(view);
                }
                Err(_) => v.failed = true,
            }
            self.scene_dirty = true;
        }
        if self.viewer.as_ref().is_some_and(|v| v.view.is_none() && !v.decoding && !v.failed && crate::storage::blob_present(&v.hash)) {
            self.request_view_decode();
        }
    }

    /// Save the open viewer's / reader's file to Downloads and toast the result — the Save pill and the view's export request share it.
    pub(super) fn viewer_save(&mut self) {
        let target = self.viewer.as_ref().map(|v| (v.hash, v.name.clone())).or_else(|| self.reader.as_ref().map(|r| (r.hash, r.name.clone())));
        if let Some((hash, name)) = target {
            self.ready_toast = Some(match self.attach_save(&name, &hash) {
                Some(dest) => tr(Msg::SavedTo(&dest)).into_owned(),
                None => tr(Msg::SaveFailed).into_owned(),
            });
            self.ready_toast_screen = None;
        }
    }

    /// Step the viewer to the previous (−1) or next (+1) image row in the conversation.
    pub(super) fn viewer_step(&mut self, dir: i32) {
        let Some(v) = self.viewer.as_ref() else {
            return;
        };
        let (contact, cur) = (v.contact, v.hash);
        let Some(ci) = self.ci_of(&contact) else { return };
        let images: Vec<[u8; 32]> = self
            .conv_of(ci)
            .map(|c| {
                c.messages
                    .iter()
                    .filter(|m| !m.deleted && m.attach.is_some_and(|a| a.kind.is_image()))
                    .filter_map(|m| m.file_parts().map(|(h, _, _)| h))
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
                m.file_parts().is_some_and(|(h, _, _)| h == *hash)
                    && (crate::types::parse_micro_image(&m.preview).is_some()
                        || m.attach.and_then(|a| a.preview_hash).is_some_and(|ph| matches!(self.img_cache.get(&ph), Some(Some(_)))))
            })
        })
    }

    /// Close whichever overlay is open (the view and its decode go with it — up to 50 MB on a phone). Returns true when one was.
    pub(super) fn close_viewers(&mut self) -> bool {
        let was = self.viewer.is_some() || self.reader.is_some();
        self.viewer = None;
        self.reader = None;
        if was {
            self.scene_dirty = true;
        }
        was
    }

    /// PRESENCE PROBES OFF THE UI THREAD (field 2026-09-18, the ANR): every hash the render asked about with no cached answer goes to the seal worker in one job; when it reports back the wrap re-measures (a band may have appeared) and the screen repaints. The render never waits on the vault for a presence answer again.
    pub(super) fn drain_presence_probes(&mut self) {
        let mut landed = false;
        while self.presence_rx.try_recv().is_ok() {
            landed = true;
        }
        if landed {
            self.msg_wrap = None;
            self.scene_dirty = true;
        }
        let wanted = crate::storage::take_presence_wanted();
        if wanted.is_empty() {
            return;
        }
        let tx = self.presence_tx.clone();
        let wake = self.event_proxy.clone();
        queue_job(&self.seal_job_tx, move || {
            for h in &wanted {
                crate::storage::blob_present_probe_now(h);
            }
            let _ = tx.send(());
            if let Some(w) = wake.as_ref() {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });
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
