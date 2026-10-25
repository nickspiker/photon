//! KEEP A SHARE OF THE DISK FREE (Nick 2026-10-21: "per device, yes, and I'd just keep it a reasonable amount of current free space").
//!
//! The rule: free space on the vault's volume, plus twice the vault's own dead space, stays at or above an eighth of the volume.
//! Twice, because content lands in both mirror rings, and the vault reuses its dead space before it grows.
//! Checked only at edges (the session coming up, a recording kept here, a fetched recording landing), never on a timer.
//! Below the floor the device stops pulling recordings in, and evicts its oldest unstarred recordings, but only ones a sibling confirms it holds (the holdings ask, holdings.rs): the fleet never loses its last copy to free one device's disk.
//! An evicted recording keeps its row; its hash joins the device-local `space.evicted` list so replication never fetches it straight back, and a deliberate fetch (Play, Save, a sibling's fetch hint) takes it off the list.
//! Waves are the only recordings today; beam video joins as its own kind when beams record (docs/beams.md stage 6).

use super::*;
use crate::network::fgtw::protocol::HoldingsKind;
use crate::linear_map::SortedSet;

/// The device-local list of recordings evicted here (concatenated hashes).
const EVICTED: &str = "space.evicted";

/// The floor is one part in this many of the volume.
const FLOOR_SHARE: u64 = 8;

/// One look at the disk, taken on a worker.
pub(super) struct SpaceReading {
    disk_free: u64,
    disk_total: u64,
    vault_dead: u64,
}

/// An eviction in flight: the candidates asked about, oldest first, and how much is still wanted.
pub(super) struct SpaceSweep {
    nonce: [u8; 32],
    candidates: Vec<([u8; 32], u64)>,
    asked: Vec<[u8; 32]>,
    need: u64,
    freed: u64,
}

/// Room below the floor, in disk bytes: zero when the reading clears it.
fn shortfall(r: &SpaceReading) -> u64 {
    let free = r.disk_free.saturating_add(r.vault_dead.saturating_mul(2));
    (r.disk_total / FLOOR_SHARE).saturating_sub(free)
}

/// Oldest recordings first until their disk cost (twice their size, one per mirror ring) covers `need`.
fn pick(mut recs: Vec<(i64, [u8; 32], u64)>, need: u64) -> Vec<([u8; 32], u64)> {
    recs.sort_unstable();
    let mut sum = 0u64;
    let mut out = Vec::new();
    for (_, h, size) in recs {
        if sum >= need {
            break;
        }
        sum = sum.saturating_add(size.saturating_mul(2));
        out.push((h, size));
    }
    out
}

/// The disk and the vault, read together (a filesystem call and a librarian round trip, so worker only).
fn read_space() -> Option<SpaceReading> {
    let (disk_free, disk_total) = crate::storage::vault_volume_space()?;
    let vault_dead = crate::storage::vault_stats_view().map_or(0, |v| v.engine.tract_blocks.saturating_sub(v.engine.live_blocks).saturating_mul(v.engine.block_bytes));
    Some(SpaceReading { disk_free, disk_total, vault_dead })
}

impl PhotonApp {
    /// Look at the disk on a worker; [`Self::drain_space`] acts on the reading. A look already in flight takes this edge with it as one more look after.
    pub(super) fn space_check(&mut self, why: &str) {
        if self.space_rx.is_some() {
            self.space_recheck = true;
            return;
        }
        crate::logf!("SPACE: checking the disk ({})", why);
        self.space_rx = Some(self.space_spawn(Vec::new()));
    }

    /// The worker behind every look: delete `evict` first (an eviction's own look follows its deletes), then read.
    fn space_spawn(&self, evict: Vec<[u8; 32]>) -> std::sync::mpsc::Receiver<Option<SpaceReading>> {
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.event_proxy.clone();
        let _ = std::thread::Builder::new().name("space".into()).spawn(move || {
            for h in &evict {
                crate::storage::blob_delete(h);
            }
            let _ = tx.send(read_space());
            if let Some(w) = wake {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });
        rx
    }

    /// The space edges, once per tick: the first tick with a session looks once, and a landed reading is acted on. True when something changed.
    pub(super) fn drain_space(&mut self) -> bool {
        if !self.space_launch_checked && self.session.is_some() && self.fleet_settings.is_some() {
            self.space_launch_checked = true;
            self.space_check("session up");
        }
        let landed = match self.space_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(r)) => Some(r),
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => Some(None),
            _ => None,
        };
        let Some(reading) = landed else { return false };
        self.space_rx = None;
        self.space_apply(reading);
        if std::mem::take(&mut self.space_recheck) {
            self.space_check("again, an edge came in during the last look");
        }
        true
    }

    fn space_apply(&mut self, reading: Option<SpaceReading>) {
        let Some(r) = reading else {
            crate::log("SPACE: the disk could not be read — no change");
            return;
        };
        let need = shortfall(&r);
        let short = need > 0;
        if short != self.space_short {
            crate::logf!(
                "SPACE: {} free of {} on the vault's volume, {} dead inside the vault — {}",
                r.disk_free,
                r.disk_total,
                r.vault_dead,
                if short { "below the floor: recordings stop coming in" } else { "above the floor: recordings come in again" }
            );
            if !short {
                self.replicate_dirty = true;
            }
        }
        self.space_short = short;
        if !short || self.space_sweep.is_some() {
            return;
        }
        let recs: Vec<(i64, [u8; 32], u64)> = self
            .conversations
            .iter()
            .flat_map(|c| c.messages.iter())
            .filter(|m| !m.deleted && m.is_wave_recording() && m.star_osc <= 0)
            .filter_map(|m| m.file.as_ref().map(|f| (m.timestamp, f.hash, f.size)))
            .filter(|(_, h, _)| !self.space_evicted.contains(h) && crate::storage::blob_present_known(h) == Some(true))
            .collect();
        let candidates = pick(recs, need);
        if candidates.is_empty() {
            crate::logf!("SPACE: {} bytes short and no unstarred recording here to let go", need);
            return;
        }
        let siblings: Vec<[u8; 32]> = self
            .contacts
            .iter()
            .filter(|c| c.is_sibling && !c.locked_out && c.is_online)
            .filter_map(|c| c.device_key())
            .collect();
        let key = self.fleet_key_ram.lock().ok().and_then(|g| *g);
        let me = self.device_keypair.as_ref().map(|kp| (kp.public.to_bytes(), *kp.secret.as_bytes()));
        let (Some(key), Some((pk, sk)), false) = (key, me, siblings.is_empty()) else {
            crate::logf!("SPACE: {} bytes short — no sibling online to confirm a second copy, so every recording stays", need);
            return;
        };
        let nonce = rand::random::<[u8; 32]>();
        let list: Vec<u8> = candidates.iter().flat_map(|(h, _)| *h).collect();
        let frame = kete::encrypt_bytes(&list, &holdings::holdings_key(&key))
            .and_then(|sealed| crate::network::fgtw::protocol::build_holdings_vsf(HoldingsKind::Ask, &nonce, sealed, &pk, &sk));
        match frame {
            Ok(frame) => {
                crate::logf!("SPACE: {} bytes short — asking {} sibling(s) which of the {} oldest recordings they hold", need, siblings.len(), candidates.len());
                self.dispatch_frame_to_sibling(frame, None);
                self.space_sweep = Some(SpaceSweep { nonce, candidates, asked: siblings, need, freed: 0 });
            }
            Err(e) => crate::logf!("SPACE: ask build failed: {}", e),
        }
    }

    /// A sibling's answer to the eviction ask: every candidate it holds may go here, oldest first, until the shortfall is covered. True when it was ours.
    pub(super) fn space_answer(&mut self, nonce: [u8; 32], sealed: &[u8], from: [u8; 32]) -> bool {
        let key = self.fleet_key_ram.lock().ok().and_then(|g| *g);
        let Some(sweep) = self.space_sweep.as_mut() else { return false };
        if sweep.nonce != nonce || !sweep.asked.contains(&from) {
            return false;
        }
        let Some(missing) = key.and_then(|k| kete::decrypt_bytes(sealed, &holdings::holdings_key(&k)).ok()).and_then(|p| holdings::split_hashes(&p)) else {
            crate::logf!("SPACE: answer from {} did not open — dropped", crate::fp(&from));
            return true;
        };
        let missing: SortedSet<[u8; 32]> = missing.into_iter().collect();
        let mut evict = Vec::new();
        sweep.candidates.retain(|(h, size)| {
            if sweep.freed >= sweep.need || missing.contains(h) {
                return true;
            }
            sweep.freed = sweep.freed.saturating_add(size.saturating_mul(2));
            evict.push(*h);
            false
        });
        let done = sweep.freed >= sweep.need || sweep.candidates.is_empty();
        if done {
            self.space_sweep = None;
        }
        if evict.is_empty() {
            crate::logf!("SPACE: {} holds none of the recordings asked about", crate::fp(&from));
            return true;
        }
        crate::logf!("SPACE: {} holds {} of them — letting this device's copies go, oldest first", crate::fp(&from), evict.len());
        self.space_evicted.extend(evict.iter().copied());
        self.persist_space_evicted();
        // The look after the deletes is the edge that clears the floor (or starts the next sweep).
        if self.space_rx.is_some() {
            self.space_recheck = true;
        }
        let rx = self.space_spawn(evict);
        if self.space_rx.is_none() {
            self.space_rx = Some(rx);
        } else {
            // A look already in flight: the deletes still run on their own worker; the recheck above reads after them.
            drop(rx);
        }
        true
    }

    /// Is this recording one the device should pull in now? Off for a device that holds no waves, below the floor, and for one evicted here.
    pub(super) fn recording_wanted(&self, hash: &[u8; 32]) -> bool {
        self.wave_hold && !self.space_short && !self.space_evicted.contains(hash)
    }

    /// A deliberate fetch takes a recording off the evicted list.
    pub(super) fn space_unevict(&mut self, hash: &[u8; 32]) {
        if self.space_evicted.remove(hash) {
            crate::logf!("SPACE: recording {}… asked for again — off the evicted list", hex::encode(&hash[..4]));
            self.persist_space_evicted();
        }
    }

    fn persist_space_evicted(&mut self) {
        if !self.ensure_fleet_settings() {
            return;
        }
        let mut list: Vec<[u8; 32]> = self.space_evicted.iter().copied().collect();
        list.sort_unstable();
        let now = vsf::eagle_time_oscillations();
        let fs = self.fleet_settings.as_mut().unwrap();
        if fs.linked(EVICTED) {
            fs.set_link(EVICTED, false, now);
        }
        if fs.set(EVICTED, vsf::VsfType::hR(list.concat()), now) {
            self.persist_and_push_settings();
        }
    }

    /// The evicted list as stored (absent = none), read where settings apply.
    pub(super) fn load_space_evicted(&mut self) {
        self.space_evicted = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.device_local(EVICTED))
            .and_then(crate::storage::fleet_settings::as_bytes)
            .and_then(|b| holdings::split_hashes(&b))
            .map(|v| v.into_iter().collect())
            .unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(free: u64, total: u64, dead: u64) -> SpaceReading {
        SpaceReading { disk_free: free, disk_total: total, vault_dead: dead }
    }

    #[test]
    fn the_floor_is_an_eighth_and_dead_vault_space_counts_twice() {
        assert_eq!(shortfall(&reading(200, 800, 0)), 0);
        assert_eq!(shortfall(&reading(60, 800, 0)), 40);
        assert_eq!(shortfall(&reading(60, 800, 20)), 0);
    }

    #[test]
    fn eviction_takes_the_oldest_until_the_shortfall_is_covered() {
        let recs = vec![(30, [3; 32], 10), (10, [1; 32], 10), (20, [2; 32], 10)];
        let got: Vec<[u8; 32]> = pick(recs, 30).into_iter().map(|(h, _)| h).collect();
        assert_eq!(got, vec![[1; 32], [2; 32]]);
        assert!(pick(Vec::new(), 30).is_empty());
    }
}
