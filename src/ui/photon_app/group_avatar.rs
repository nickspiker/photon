//! GROUP AVATARS (Nick 2026-11-02: "molecule avatars, drag/drop onto the molecule info page with the big avatar with the hover hint to drag/drop or tap on adroid to choose").
//! A group's picture is a signed roster record (`AvatarRecord`, newest wins, any member) naming a blob: the house image, AV1 inside a VSF document, the very bytes a person's avatar publishes.
//! Every member fetches that blob like any of the group's files (replicate_sweep), sealed under the group's key on the wire and held in each member's vault; FGTW never sees it.
//! This file sets and removes the picture, decodes a held one off the UI thread, and keeps one scaled copy per diameter the screen draws it at.

use super::*;
use crate::types::molecule::MoleculeId;

/// One decoded group picture: the blob it came from, its display pixels (`AVATAR_SIZE` square, the colour the screen takes), and its scaled copies.
pub(super) struct GroupFace {
    pub(super) image: [u8; 32],
    px: Vec<u8>,
    scaled: Vec<(usize, Vec<u8>)>,
}

/// A picture this device just made for a group: posted as a record once its blob is stored.
pub(super) struct GroupFaceSet {
    gid: MoleculeId,
    image: [u8; 32],
    px: Vec<u8>,
}

/// What the worker hands back to the UI thread.
pub(super) enum GroupFaceDone {
    /// A picture we chose was encoded and stored.
    Set(GroupFaceSet),
    /// A held picture was read: its pixels, or `None` when the blob is not a picture this build draws.
    Decoded([u8; 32], Option<Vec<u8>>),
}

/// How many diameters one picture keeps scaled: the Ready row, the conversation header, the Manage page, and one more for a zoom between frames. The oldest goes first.
const FACE_DIAMETERS: usize = 4;

impl PhotonApp {
    /// Set group `gid`'s picture from a picked or dropped image file. NOTHING runs on the UI thread: a low-priority worker decodes, encodes the house image, and stores the blob; the record is minted and posted when it reports back (`drain_group_faces`), so no member is told of a picture that is not stored yet.
    pub(super) fn set_molecule_avatar(&mut self, gid: MoleculeId, image_bytes: Vec<u8>) {
        let Some(seed) = self.messenger_root() else {
            crate::log("GROUP AVATAR: ignored — not attested yet");
            return;
        };
        if !self.molecule_rosters.iter().any(|(g, _)| *g == gid) {
            return;
        }
        crate::logf!("GROUP AVATAR: making {}'s picture from {} bytes", hex::encode(&gid.0[..4]), image_bytes.len());
        let tx = self.group_face_tx.clone();
        let wake = self.event_proxy.clone();
        crate::log_err!(
            std::thread::Builder::new().name("group-avatar".into()).spawn(move || {
                crate::platform::lower_this_thread();
                let rgb_f32 = match crate::ui::avatar::image_to_avatar_rgb_f32(&image_bytes) {
                    Ok(p) => p,
                    Err(e) => {
                        crate::logf!("GROUP AVATAR: decode failed: {}", e);
                        return;
                    }
                };
                let px = crate::ui::colour_convert::vsf_rgb_to_bt2020(&crate::ui::avatar::avatar_rgb_f32_to_u8(&rgb_f32));
                let doc = crate::ui::avatar::encode_avatar_rgb_f32(&rgb_f32).and_then(|av1| crate::ui::avatar_scoped::avatar_plaintext(av1, vsf::visual::Encoding::Gamma2));
                let doc = match doc {
                    Ok(d) => d,
                    Err(e) => {
                        crate::logf!("GROUP AVATAR: encode failed: {}", e);
                        return;
                    }
                };
                let image = *blake3::hash(&doc).as_bytes();
                if let Err(e) = crate::storage::blob_store(&seed, &image, &doc) {
                    crate::logf!("GROUP AVATAR: the vault refused the picture: {}", e);
                    return;
                }
                crate::log_err!(tx.send(GroupFaceDone::Set(GroupFaceSet { gid, image, px })), "hand a group picture to the UI");
                if let Some(w) = wake.as_ref() {
                    wake_ui(w);
                }
            }),
            "the group-avatar thread spawn"
        );
    }

    /// Take group `gid`'s picture away: a record with no image, newer than the one it replaces, so every member draws the members' pie again.
    pub(super) fn remove_molecule_avatar(&mut self, gid: MoleculeId) -> bool {
        self.post_molecule_avatar(gid, None)
    }

    /// Mint, merge, persist and post one avatar record (the rename pattern): the record alone rides the Records post, never the roster around it.
    fn post_molecule_avatar(&mut self, gid: MoleculeId, image: Option<[u8; 32]>) -> bool {
        let (Some(our_pid), Some(seed)) = (self.own_party_id(), self.device_seed()) else {
            return false;
        };
        let Some(pos) = self.molecule_rosters.iter().position(|(g, _)| *g == gid) else {
            return false;
        };
        let rec = crate::types::molecule::avatar_record(our_pid, image, &seed);
        let mut posted = crate::types::molecule::Roster::default();
        posted.merge_avatar(rec.clone());
        if !self.molecule_rosters[pos].1.merge_avatar(rec) {
            return false;
        }
        self.persist_molecule(&gid);
        let blob = crate::storage::molecule::roster_to_vsf_bytes(&gid, &posted).ok();
        self.queue_molecule_post(super::molecules::MoleculePost { gid, signal: crate::types::molecule::MoleculeSignal::Records, blob, wrap: None, kem: None, text: None, queued: self.tick_serial, control: false });
        match image {
            Some(h) => crate::logf!("GROUP AVATAR: {} now wears picture {}…", hex::encode(&gid.0[..4]), hex::encode(&h[..4])),
            None => crate::logf!("GROUP AVATAR: {}'s picture taken away", hex::encode(&gid.0[..4])),
        }
        self.scene_dirty = true;
        true
    }

    /// The worker's answers: a picture we made is posted; a held picture is installed (or remembered as unreadable, so it is never decoded again this session).
    pub(super) fn drain_group_faces(&mut self) {
        while let Ok(done) = self.group_face_rx.try_recv() {
            match done {
                GroupFaceDone::Set(s) => {
                    self.group_faces.retain(|f| f.image != s.image);
                    self.group_faces.push(GroupFace { image: s.image, px: s.px, scaled: Vec::new() });
                    self.post_molecule_avatar(s.gid, Some(s.image));
                }
                GroupFaceDone::Decoded(image, px) => {
                    self.group_faces_decoding.retain(|h| *h != image);
                    match px {
                        Some(px) => {
                            self.group_faces.retain(|f| f.image != image);
                            self.group_faces.push(GroupFace { image, px, scaled: Vec::new() });
                        }
                        None => {
                            crate::logf!("GROUP AVATAR: picture {}… is not one this build draws — the members' pie stays", hex::encode(&image[..4]));
                            self.group_faces_unreadable.push(image);
                        }
                    }
                    self.scene_dirty = true;
                }
            }
        }
    }

    /// One pass per edge (`group_faces_dirty`: a roster changed, a blob landed, presence answers came back): every group picture a roster names and this device holds is decoded on a worker; one no roster names any more is let go. Fetching is the replication sweep's (it names each group's picture beside the group's files).
    pub(super) fn group_faces_tick(&mut self) {
        if !std::mem::take(&mut self.group_faces_dirty) {
            return;
        }
        let Some(seed) = self.messenger_root() else { return };
        let named: Vec<[u8; 32]> = self.molecule_rosters.iter().filter_map(|(_, r)| r.avatar_image()).collect();
        self.group_faces.retain(|f| named.contains(&f.image));
        for image in named {
            if self.group_faces.iter().any(|f| f.image == image) || self.group_faces_decoding.contains(&image) || self.group_faces_unreadable.contains(&image) {
                continue;
            }
            // Presence from the cache alone (the UI thread makes no vault call): a pending probe raises the edge again when it answers, and a picture not held yet is the sweep's to fetch, whose landing raises it too.
            if crate::storage::blob_present_known(&image) != Some(true) {
                continue;
            }
            self.group_faces_decoding.push(image);
            let tx = self.group_face_tx.clone();
            let wake = self.event_proxy.clone();
            crate::log_err!(
                std::thread::Builder::new().name("group-avatar-read".into()).spawn(move || {
                    // WHY/PROOF: the picture is another member's blob — every setter encodes exactly AVATAR_SIZE square, and the scaled draw is handed that square; anything else is not a group picture and is refused, never resized into one.
                    let px = crate::storage::blob_load(&seed, &image).and_then(|b| crate::ui::avatar_scoped::decode_avatar_plaintext(&b)).filter(|(size, _)| *size == crate::ui::avatar::AVATAR_SIZE).map(|(_, px)| px);
                    crate::log_err!(tx.send(GroupFaceDone::Decoded(image, px)), "hand a group picture's pixels to the UI");
                    if let Some(w) = wake.as_ref() {
                        wake_ui(w);
                    }
                }),
                "the group-avatar read thread spawn"
            );
        }
    }
}

/// Make sure group picture `image` holds a copy scaled to `diam`. Free of `self` so the render can call it beside the chrome borrow; the draw then reads [`face_at`].
pub(super) fn scale(faces: &mut [GroupFace], image: &[u8; 32], diam: usize) {
    let Some(f) = faces.iter_mut().find(|f| f.image == *image) else { return };
    if f.scaled.iter().any(|(d, _)| *d == diam) {
        return;
    }
    if f.scaled.len() == FACE_DIAMETERS {
        f.scaled.remove(0);
    }
    let s = crate::ui::avatar_render::update_avatar_scaled(&f.px, crate::ui::avatar::AVATAR_SIZE, diam);
    f.scaled.push((diam, s));
}

/// Group picture `image` scaled to `diam`, when it is decoded and scaled there.
pub(super) fn face_at<'a>(faces: &'a [GroupFace], image: &[u8; 32], diam: usize) -> Option<&'a [u8]> {
    faces.iter().find(|f| f.image == *image)?.scaled.iter().find(|(d, _)| *d == diam).map(|(_, s)| s.as_slice())
}
