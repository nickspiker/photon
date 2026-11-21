//! CONTACTS FIRST, ROWS AFTER (the owner 2026-11-20: "load the contacts first, then show it, then avatars, then show it", messages after that).
//!
//! The resume used to read every stored row of every conversation on the UI thread before the first Ready frame: 1245 ms of a 1526 ms resume on the Pro (ferros, 2026-11-18, 18 contacts, about 2300 rows), with the launch screen saying "waiting for your identity" the whole time.
//! Now the vault worker opens the vault and reads the contacts and each conversation's summary (its durable state and its newest visible row, which is all the contact list draws and sorts by), the UI goes to Ready on that, and this module's worker reads the rows after.
//!
//! The order: newest-active conversation first; a conversation someone opens jumps the queue and its newest page lands at once, the rest of it right behind.
//! Nothing waits on all the rows. Everything that would read a partial row set as the whole waits for ITS conversation's load edge (`Conversation::rows_pending`): the receive arm and the ACK arm park their frames, history pages park, a persist parks, a held-row flush is owed, the sync digest advertises nothing, the history walk and the anti-entropy compare skip it.
//! The sweeps that are about every row at once (the keep trim, the space floor, the checkpoint root, the holdings list, the self-stub purge) run on the edge the last conversation lands.

use super::PhotonApp;
use crate::network::status::StatusUpdate;
use crate::storage::contacts::{RowKey, RowLoadTiming};
use crate::storage::{FlatStorage, StorageError};
use crate::types::{ChatMessage, Contact, Conversation, ConversationId};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// One page of a conversation's rows, as the loader reads them newest first: enough to fill an opened conversation's screen, few enough that an ask for another conversation waits on at most one page of this one.
const ROWS_PAGE: usize = 1 << 7;

/// The ids the vault worker derives each conversation from (the same derivation `conv_mut_of` uses): a friend's conversation is ours by our identity party id, a sibling's by this device's sibling party id.
#[derive(Clone, Copy)]
pub(super) struct ResumePids {
    pub handle_proof: [u8; 32],
    pub own: Option<[u8; 32]>,
    pub sibling: Option<[u8; 32]>,
}

/// What the vault worker hands the UI for the first Ready frame: the vault, the people, and each conversation as the list draws it.
pub(super) struct ResumeOpened {
    pub storage: Arc<FlatStorage>,
    pub contacts: Vec<Contact>,
    pub siblings: Vec<Contact>,
    /// One per contact and sibling: the durable state (unread, the history cursor) and the head rows, `rows_pending` set.
    pub conversations: Vec<Conversation>,
    /// Each group's head rows, by its conversation id (its conversation itself is built from its roster on the UI thread).
    pub group_heads: Vec<(ConversationId, Vec<ChatMessage>)>,
    pub ms_open: u128,
    pub ms_contacts: u128,
    pub ms_summaries: u128,
}

/// The vault worker's whole job, run on the worker (or inline when no thread could be spawned): the open, then the contacts, then every conversation's summary.
pub(super) fn open_and_load_contacts(opened: Result<Arc<FlatStorage>, StorageError>, pids: &ResumePids, t_boot: Instant) -> Result<ResumeOpened, StorageError> {
    let s = opened?;
    let ms_open = t_boot.elapsed().as_millis();
    let t = Instant::now();
    let contacts = crate::storage::contacts::load_all_contacts(&s);
    let siblings = crate::storage::contacts::load_all_siblings(pids.handle_proof, &s);
    let ms_contacts = t.elapsed().as_millis();
    let t = Instant::now();
    let mut conversations: Vec<Conversation> = Vec::new();
    for c in contacts.iter().chain(siblings.iter()) {
        let Some(us) = (if c.is_sibling { pids.sibling } else { pids.own }) else {
            continue;
        };
        let mut conv = c.conversation(&us);
        if conversations.iter().any(|v| v.id() == conv.id()) {
            continue;
        }
        crate::storage::contacts::load_conversation_state(&mut conv, &c.handle_hash, &s);
        // A sibling's conversation is the bridge terminal: never listed, so it needs no head.
        if !c.is_sibling {
            match crate::storage::contacts::load_head_rows(conv.id().as_bytes(), &s) {
                Ok(head) => conv.messages = head,
                Err(e) => crate::logf!("UI: resume could not read the newest row of {}: {}", crate::fp(&c.handle_proof), e),
            }
        }
        conv.rows_pending = true;
        conversations.push(conv);
    }
    let group_heads = match crate::storage::molecule::load_molecule_list(&s) {
        Ok(ids) => ids
            .into_iter()
            .filter_map(|g| match crate::storage::contacts::load_head_rows(&g.0, &s) {
                Ok(head) => Some((ConversationId::from_bytes(g.0), head)),
                Err(e) => {
                    crate::logf!("MOLECULE: could not read the newest row of {}: {}", hex::encode(&g.0[..4]), e);
                    None
                }
            })
            .collect(),
        Err(_) => Vec::new(), // load_all_molecules reads the same index on the UI thread and says why it failed
    };
    let ms_summaries = t.elapsed().as_millis();
    Ok(ResumeOpened { storage: s, contacts, siblings, conversations, group_heads, ms_open, ms_contacts, ms_summaries })
}

/// The newest visible row's time in a row set: the contact list's sort key.
pub(super) fn newest_visible(rows: &[ChatMessage]) -> Option<i64> {
    rows.iter().filter(|m| !m.is_control() && !m.deleted).map(|m| m.timestamp).max()
}

/// What the row worker posts back.
pub(super) enum Landed {
    /// The newest page of a conversation someone opened before its rows landed: shown at once, the rest right behind.
    Page { id: ConversationId, rows: Vec<ChatMessage> },
    /// Every stored row of a conversation, oldest first, or why the read failed.
    Full { id: ConversationId, rows: Result<Vec<ChatMessage>, String>, timing: RowLoadTiming },
}

/// The UI's half of the load: the worker's channels, what waits on which conversation, and the spend for the PERF line.
pub(super) struct RowsLoad {
    ask: mpsc::Sender<ConversationId>,
    rx: mpsc::Receiver<Landed>,
    left: usize,
    jobs: usize,
    since: Instant,
    timing: RowLoadTiming,
    install: Duration,
    first_page: Option<Duration>,
    asked: Vec<ConversationId>,
    frames: Vec<(ConversationId, StatusUpdate)>,
    pages: Vec<(ConversationId, super::HistPageOpened)>,
    persists: Vec<(ConversationId, Vec<i64>)>,
    resends: Vec<ConversationId>,
}

/// One conversation's read in progress on the worker: its keys, how far down from the newest it has read, and the pages so far (newest first).
struct Job {
    id: ConversationId,
    keys: Option<Vec<RowKey>>,
    hi: usize,
    pages: Vec<Vec<ChatMessage>>,
    legacy: Vec<rarangi::Pk<'static>>,
    timing: RowLoadTiming,
    paged: bool,
}

/// The row worker: reads each conversation a page at a time, newest first, and takes asks between pages.
pub(super) fn rows_worker(storage: Arc<FlatStorage>, ids: Vec<ConversationId>, ask: mpsc::Receiver<ConversationId>, tx: mpsc::Sender<Landed>, wake: impl Fn()) {
    let mut queue: std::collections::VecDeque<Job> = ids
        .into_iter()
        .map(|id| Job { id, keys: None, hi: 0, pages: Vec::new(), legacy: Vec::new(), timing: RowLoadTiming::default(), paged: false })
        .collect();
    loop {
        // An ask between pages: the asked conversation goes to the front, and its newest page goes out the moment it is read (or now, when it is already read).
        while let Ok(id) = ask.try_recv() {
            let Some(p) = queue.iter().position(|j| j.id == id) else { continue };
            let mut j = queue.remove(p).expect("position found it");
            if !j.paged {
                j.paged = true;
                if let Some(newest) = j.pages.first() {
                    if tx.send(Landed::Page { id, rows: newest.clone() }).is_err() {
                        return;
                    }
                    wake();
                }
            }
            queue.push_front(j);
        }
        let Some(mut job) = queue.pop_front() else { return };
        let table = *job.id.as_bytes();
        if job.keys.is_none() {
            match crate::storage::contacts::message_keys(&table, &storage, &mut job.timing) {
                Ok(k) => {
                    job.hi = k.len();
                    job.keys = Some(k);
                }
                Err(e) => {
                    if tx.send(Landed::Full { id: job.id, rows: Err(e.to_string()), timing: job.timing }).is_err() {
                        return;
                    }
                    wake();
                    continue;
                }
            }
        }
        let keys = job.keys.as_ref().expect("read just above");
        // WHY: the oldest page of a table is usually shorter than ROWS_PAGE. PROOF: `hi - ROWS_PAGE` with hi < ROWS_PAGE underflows usize (a panic, or a wrapped slice start far out of bounds); the min keeps the page start at 0 there.
        let lo = job.hi - ROWS_PAGE.min(job.hi);
        match crate::storage::contacts::read_message_rows(&table, &keys[lo..job.hi], &storage, &mut job.legacy, &mut job.timing) {
            Ok(rows) => {
                if job.paged && job.pages.is_empty() {
                    if tx.send(Landed::Page { id: job.id, rows: rows.clone() }).is_err() {
                        return;
                    }
                    wake();
                }
                job.pages.push(rows);
                job.hi = lo;
            }
            Err(e) => {
                if tx.send(Landed::Full { id: job.id, rows: Err(e.to_string()), timing: job.timing }).is_err() {
                    return;
                }
                wake();
                continue;
            }
        }
        if job.hi > 0 {
            queue.push_front(job);
            continue;
        }
        // The whole table is read: oldest page first, the flag-day purge, the blank twins folded (load_rows_of's tail).
        let mut rows: Vec<ChatMessage> = job.pages.drain(..).rev().flatten().collect();
        if let Err(e) = crate::storage::contacts::purge_legacy_rows(&table, &job.legacy, &storage) {
            crate::logf!("STORAGE: the flag-day purge of conversation {} failed: {}", hex::encode(&table[..4]), e);
        }
        let twins = crate::types::conversation::collapse_bare_twins_in(&mut rows);
        if twins > 0 {
            crate::logf!("STORAGE: {} blank twin row(s) of file photons folded away for conversation {}", twins, hex::encode(&table[..4]));
        }
        if tx.send(Landed::Full { id: job.id, rows: Ok(rows), timing: job.timing }).is_err() {
            return;
        }
        wake();
    }
}

impl PhotonApp {
    /// The ids the vault worker needs, read before it spawns (the session or PhotonKeys, and this device's key).
    pub(super) fn resume_pids(&self) -> ResumePids {
        ResumePids { handle_proof: self.our_handle_proof().unwrap_or_default(), own: self.own_party_id(), sibling: self.our_sibling_pid() }
    }

    /// Start the row load for `ids`, in that order, off the UI thread. With no thread to be had, they load inline, the old way, rather than never.
    pub(super) fn start_rows_load(&mut self, ids: Vec<ConversationId>) {
        let Some(storage) = self.storage.clone() else { return };
        if ids.is_empty() {
            return;
        }
        let (ask_tx, ask_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let wake = self.event_proxy.clone();
        let jobs = ids.len();
        let worker_ids = ids.clone();
        let spawned = std::thread::Builder::new()
            .name("rows".into())
            .spawn(move || {
                rows_worker(storage, worker_ids, ask_rx, tx, || {
                    if let Some(w) = wake.as_ref() {
                        super::wake_ui(w);
                    }
                })
            })
            .is_ok();
        // A load still running (a second resume before the first one's rows landed): what waits on it carries over, and its conversations are in `ids` again, since they are still pending.
        let carried = self.rows_load.take();
        let (frames, pages, persists, resends) = carried.map_or_else(Default::default, |c| (c.frames, c.pages, c.persists, c.resends));
        self.rows_load = Some(RowsLoad {
            ask: ask_tx,
            rx,
            left: jobs,
            jobs,
            since: Instant::now(),
            timing: RowLoadTiming::default(),
            install: Duration::ZERO,
            first_page: None,
            asked: Vec::new(),
            frames,
            pages,
            persists,
            resends,
        });
        if spawned {
            return;
        }
        crate::log("ROWS: no thread for the row load; reading them here");
        let Some(storage) = self.storage.clone() else { return };
        for id in ids {
            let mut timing = RowLoadTiming::default();
            let rows = crate::storage::contacts::load_rows_of(id.as_bytes(), &storage, &mut timing).map_err(|e| e.to_string());
            self.rows_landed(id, rows, timing);
        }
    }

    /// True until the last conversation's rows land.
    pub(super) fn rows_loading(&self) -> bool {
        self.rows_load.is_some()
    }

    /// The conversation a frame on `token` lands in, while its rows are still loading.
    pub(super) fn rows_pending_for_token(&self, token: &[u8; 32]) -> Option<ConversationId> {
        self.rows_load.as_ref()?;
        let fid = self.friendship_chains.iter().find(|(_, c)| c.conversation_token == *token).map(|(id, _)| *id)?;
        self.conversations.iter().any(|v| v.id() == fid && v.rows_pending).then_some(fid)
    }

    /// The conversation a history page on `token` merges into, while its rows are still loading.
    pub(super) fn rows_pending_for_page(&self, token: &[u8; 32]) -> Option<ConversationId> {
        self.rows_load.as_ref()?;
        let id = self.conversation_for_token(token)?;
        self.conversations.iter().any(|v| v.id() == id && v.rows_pending).then_some(id)
    }

    /// Someone needs this conversation's rows now (it opened, or something waits on it): it jumps the worker's queue, newest page first.
    pub(super) fn want_rows_now(&mut self, id: ConversationId) {
        let Some(load) = self.rows_load.as_mut() else { return };
        if load.asked.contains(&id) || !self.conversations.iter().any(|v| v.id() == id && v.rows_pending) {
            return;
        }
        load.asked.push(id);
        crate::log_err!(load.ask.send(id), "ask the row worker for a conversation first");
    }

    /// Frames the receive and ACK arms parked because their conversation's rows are still loading: replayed thru the arm, gates and all, on that conversation's load edge.
    pub(super) fn hold_frames_for_rows(&mut self, held: Vec<(ConversationId, StatusUpdate)>) {
        if held.is_empty() {
            return;
        }
        let mut ids: Vec<ConversationId> = held.iter().map(|(id, _)| *id).collect();
        ids.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        ids.dedup();
        crate::logf!("ROWS: {} frame(s) wait for the rows of {} conversation(s)", held.len(), ids.len());
        match self.rows_load.as_mut() {
            Some(load) => load.frames.extend(held),
            None => self.chat_replay_queue.extend(held.into_iter().map(|(_, u)| u)),
        }
        for id in ids {
            self.want_rows_now(id);
        }
    }

    /// A history page whose conversation's rows are still loading: merged on that conversation's load edge (a merge against a partial row set reads every stored row as new, and rings for them).
    pub(super) fn hold_page_for_rows(&mut self, id: ConversationId, page: super::HistPageOpened) {
        match self.rows_load.as_mut() {
            Some(load) => load.pages.push((id, page)),
            None => {
                crate::log_err!(self.hist_opened_tx.send(page), "requeue a history page");
            }
        }
        self.want_rows_now(id);
    }

    /// A persist of a conversation whose rows are still loading: written on its load edge, so the snapshot carries every stored row and no signalled row is refused.
    pub(super) fn hold_persist_for_rows(&mut self, id: ConversationId, signal_rows: Vec<i64>) {
        match self.rows_load.as_mut() {
            Some(load) => match load.persists.iter_mut().find(|(i, _)| *i == id) {
                Some((_, rows)) => rows.extend(signal_rows),
                None => load.persists.push((id, signal_rows)),
            },
            None => return self.persist_conversation_signalled(id, signal_rows),
        }
        self.want_rows_now(id);
    }

    /// A held-row flush of a conversation whose rows are still loading: owed until they land (the undelivered rows it re-queues are in them).
    pub(super) fn owe_resend_for_rows(&mut self, id: ConversationId) {
        match self.rows_load.as_mut() {
            Some(load) if !load.resends.contains(&id) => load.resends.push(id),
            Some(_) => {}
            None => return self.resend_held_rows(id),
        }
        self.want_rows_now(id);
    }

    /// The row worker's landings, once per tick: pages shown, conversations hydrated and released, and the edge the last one lands.
    pub(super) fn drain_rows_landed(&mut self) -> bool {
        let Some(load) = self.rows_load.as_ref() else { return false };
        let landed: Vec<Landed> = load.rx.try_iter().collect();
        if landed.is_empty() {
            return false;
        }
        for l in landed {
            match l {
                Landed::Page { id, rows } => {
                    let t = Instant::now();
                    if let Some(conv) = self.conversations.iter_mut().find(|v| v.id() == id && v.rows_pending) {
                        for m in rows {
                            conv.insert_message_sorted(m);
                        }
                    }
                    if let Some(load) = self.rows_load.as_mut() {
                        load.install += t.elapsed();
                        if load.first_page.is_none() {
                            load.first_page = Some(load.since.elapsed());
                        }
                    }
                    self.scene_dirty = true;
                }
                Landed::Full { id, rows, timing } => self.rows_landed(id, rows, timing),
            }
        }
        true
    }

    /// One conversation's rows landed (or failed to): hydrate it, release everything that waited on it, and close the load when it was the last.
    fn rows_landed(&mut self, id: ConversationId, rows: Result<Vec<ChatMessage>, String>, timing: RowLoadTiming) {
        let t = Instant::now();
        if let Some(conv) = self.conversations.iter_mut().find(|v| v.id() == id) {
            match rows {
                Ok(rows) => {
                    conv.adopt_loaded_rows(rows);
                    // RAM now holds every stored row plus what arrived meanwhile: this licenses a persist (load_messages' rule).
                    conv.hydrated = true;
                }
                // Left un-hydrated, as a failed load always was: the persist path's late-hydration rescue retries the read before anything writes.
                Err(e) => crate::logf!("UI: resume failed to load messages for conversation {}: {}", hex::encode(&id.as_bytes()[..4]), e),
            }
            conv.rows_pending = false;
        }
        self.census_line(id);
        let (frames, pages, persists, resends) = match self.rows_load.as_mut() {
            Some(load) => {
                load.left -= 1; // one Full per job, each job queued once
                load.timing.add(&timing);
                let frames: Vec<StatusUpdate> = take_for(&mut load.frames, id).into_iter().map(|(_, u)| u).collect();
                let pages: Vec<super::HistPageOpened> = take_for(&mut load.pages, id).into_iter().map(|(_, p)| p).collect();
                let persists: Vec<Vec<i64>> = take_for(&mut load.persists, id).into_iter().map(|(_, r)| r).collect();
                let resend = load.resends.contains(&id);
                load.resends.retain(|r| *r != id);
                (frames, pages, persists, resend)
            }
            None => (Vec::new(), Vec::new(), Vec::new(), false),
        };
        if !frames.is_empty() {
            crate::logf!("ROWS: {} frame(s) for conversation {} replayed on its rows", frames.len(), hex::encode(&id.as_bytes()[..4]));
        }
        self.chat_replay_queue.extend(frames);
        for p in pages {
            crate::log_err!(self.hist_opened_tx.send(p), "release a history page that waited on its rows");
        }
        for signal_rows in persists {
            self.persist_conversation_signalled(id, signal_rows);
        }
        if resends {
            self.resend_held_rows(id);
        }
        // The digest this conversation advertises is real now.
        self.update_sync_records();
        // A chainless origin's undelivered forwards in this conversation, which the sibling-online sweep could not see while the rows loaded.
        if let Some(ci) = self.pair_contact_of(id) {
            self.reserve_fleet_forward_for(ci);
        }
        self.scene_dirty = true;
        if let Some(load) = self.rows_load.as_mut() {
            load.install += t.elapsed();
        }
        if self.rows_load.as_ref().is_some_and(|l| l.left == 0) {
            self.rows_all_landed();
        }
    }

    /// STORAGE CENSUS (field diagnosis, 2026-08-10): one line per contact naming the conversation table and how many rows loaded from it, printed as its rows land. The "messages don't recover" round showed devices advertising 92 rows while serving 3 — the row sets had split across contact keys, and nothing in the logs could say WHICH table held what. Delete once the split-conversation incident is closed.
    fn census_line(&self, id: ConversationId) {
        let Some(rows) = self.conversations.iter().find(|v| v.id() == id).map(|v| v.messages.len()) else { return };
        for c in self.contacts.iter().filter(|c| self.our_party_id(c).is_some_and(|us| c.conversation(&us).id() == id)) {
            // The row's full identity beside its table: handle_hash names the party id the tokens/tables derive from, first-met names the pinned device, state names the ceremony posture (the 2026-08-12 evening round: fp+table alone can't tell a stale-keyed self row, a debris row and a sibling apart).
            // The §4.2 owner claim, named (Emma's ghost, 2026-09-08: a one-device fleet rendered "secured on <phantom>" — a foreign/zero owner pubkey nobody could identify remotely because no log ever printed it).
            let detail = format!(
                " [id {} hash {} first-met {} {:?}{}{}]",
                hex::encode(&c.id.as_bytes()[..4]),
                hex::encode(&c.handle_hash[..4]),
                hex::encode(&c.device_key().unwrap_or_default()[..4]),
                c.clutch_state,
                if c.is_sibling { " sibling" } else { "" },
                c.ceremony_owner.map_or(String::new(), |o| format!(" owner {}{}", hex::encode(&o[..4]), if c.owner_woven { " woven" } else { "" }))
            );
            crate::logf!("STORAGE: census — {} table {} holds {} row(s) in RAM after load{}", crate::fp(&c.handle_proof), hex::encode(&id.as_bytes()[..4]), rows, detail);
        }
    }

    /// The last conversation's rows landed: the PERF line, then the sweeps that are about every row at once.
    fn rows_all_landed(&mut self) {
        let Some(load) = self.rows_load.take() else { return };
        let t = &load.timing;
        let ms = |d: Duration| d.as_millis();
        let read = t.catalog + t.vault + t.decode + t.build;
        let wall = load.since.elapsed();
        crate::logf!(
            "PERF: rows after Ready — {} row(s) of {} conversation(s) in {}ms: catalog {}ms, vault read+decrypt {}ms, record decode {}ms, row build {}ms (worker); install {}ms (UI thread); the rest {}ms is the queue and the wakes{}",
            t.rows,
            load.jobs,
            ms(wall),
            ms(t.catalog),
            ms(t.vault),
            ms(t.decode),
            ms(t.build),
            ms(load.install),
            // WHY: the worker's read and the UI's install overlap the wall clock, so their sum may pass it on a fast landing. PROOF: wall - (read + install) underflows Duration there, which panics.
            ms(wall.saturating_sub(read + load.install)),
            load.first_page.map_or(String::new(), |d| format!("; an opened conversation's newest page showed {}ms after Ready", ms(d)))
        );
        // Anything still held names a conversation no job loaded: it goes on as it would have, unheld.
        self.chat_replay_queue.extend(load.frames.into_iter().map(|(_, u)| u));
        for (_, p) in load.pages {
            crate::log_err!(self.hist_opened_tx.send(p), "release a history page");
        }
        for (id, rows) in load.persists {
            self.persist_conversation_signalled(id, rows);
        }
        for id in load.resends {
            self.resend_held_rows(id);
        }
        self.purge_empty_self_stubs();
        self.keep_trim("the rows loaded");
        // A Release armed while the rows loaded: its holdings check, refused then, runs now.
        if self.settings_removeshred_armed && self.holdings_check.is_none() {
            self.holdings_check_start();
        }
        if self.space_short {
            self.space_check("the rows loaded");
        }
    }

    /// SELF-STUB PURGE: a non-sibling row carrying OUR handle_proof under a handle_hash that is NOT our identity pid is a corrupt self-contact stub — the source of the "CLUTCH offers toward its OWN identity" storm (ticketed 2026-08-07; the census caught THREE self rows on one desktop, 2026-08-11, two of them empty stubs spraying 573KB offers at the fleet's own contacts).
    /// Empty-conversation stubs only, so it waits for every row: a row that somehow holds messages is somebody's data and stays for a deliberate repair, never a boot-time sweep. The vault writes run off the UI thread.
    fn purge_empty_self_stubs(&mut self) {
        let (Some(our_proof), Some(our_pid), Some(storage)) = (self.our_handle_proof(), self.own_party_id(), self.storage.clone()) else {
            return;
        };
        let stub_hashes: Vec<[u8; 32]> = (0..self.contacts.len())
            .filter(|&ci| {
                let c = &self.contacts[ci];
                !c.is_sibling && c.handle_proof == our_proof && c.handle_hash != our_pid && self.conv_of(ci).map_or(true, |v| v.messages.is_empty())
            })
            .map(|ci| self.contacts[ci].handle_hash)
            .collect();
        if stub_hashes.is_empty() {
            return;
        }
        for hh in &stub_hashes {
            crate::logf!("STORAGE: census — PURGING empty self-contact stub (key {}…) — its ceremony queue dies with it", hex::encode(&hh[..4]));
        }
        self.contacts.retain(|c| !stub_hashes.contains(&c.handle_hash));
        // Rewrite the index too, or the next launch resurrects the stubs from the list (same rule as the ostracism path).
        let index: Vec<crate::storage::contacts::ContactIdentity> = self
            .contacts
            .iter()
            .filter(|c| !c.is_sibling)
            .map(|c| crate::storage::contacts::ContactIdentity { handle_proof: c.handle_proof, party_id: c.handle_hash, avatar_pin: c.avatar_pin })
            .collect();
        crate::log_err!(
            std::thread::Builder::new().name("stub-purge".into()).spawn(move || {
                for hh in &stub_hashes {
                    crate::log_err!(crate::storage::contacts::delete_contact(hh, &storage), "an empty self-contact stub's removal");
                }
                if let Err(e) = crate::storage::contacts::save_contact_list(&index, &storage) {
                    crate::logf!("STORAGE: census — stub-purge index rewrite failed: {}", e);
                }
            }),
            "spawn the self-stub purge"
        );
    }
}

/// Take every item held for `id` out of `held`, in order.
fn take_for<T>(held: &mut Vec<(ConversationId, T)>, id: ConversationId) -> Vec<(ConversationId, T)> {
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(held).into_iter().partition(|(i, _)| *i == id);
    *held = rest;
    mine
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::contacts::{load_head_rows, save_contact, save_conversation_state, save_messages};
    use crate::types::DevicePubkey;

    /// The Pro's tables on 2026-11-18 (the census, rows per conversation): 15 friends, the three that hold nothing included, about 2300 rows.
    const OWNER_ROWS: [usize; 15] = [244, 341, 939, 283, 63, 0, 1, 0, 53, 0, 25, 0, 211, 132, 47];

    /// Our party id in the synthetic vault.
    const US: [u8; 32] = [0xAA; 32];

    /// A friend's party id, handle proof and device key, one byte apart per friend.
    fn friend(b: u8) -> Contact {
        let mut hh = [0u8; 32];
        hh[0] = b;
        hh[1] = 0x5E;
        let mut proof = [0u8; 32];
        proof[0] = b;
        proof[1] = 0x9F;
        let mut dev = [0u8; 32];
        dev[0] = b;
        dev[1] = 0xD0;
        Contact::from_pin([0u8; 64], proof, hh, Some(DevicePubkey::from_bytes(dev)))
    }

    /// `n` rows of a conversation, oldest first: text of a chat's lengths, both directions, a control row on top when `control_head`.
    fn rows(seed: u8, n: usize, control_head: bool) -> Vec<ChatMessage> {
        let base: i64 = 1 << 40;
        let mut out = Vec::with_capacity(n + 1);
        for i in 0..n {
            let h = blake3::hash(&[seed, (i >> 8) as u8, i as u8]);
            let len = (1 << 4) + (h.as_bytes()[0] as usize) % (1 << 7); // a chat line, 16 to 143 bytes
            let text: String = h.to_hex().chars().cycle().take(len).collect();
            let mut m = ChatMessage::new_with_timestamp(text, h.as_bytes()[1] & 1 == 0, base + (i as i64) * (1 << 20));
            m.delivered = m.is_outgoing;
            out.push(m);
        }
        if control_head && n > 0 {
            let wave = crate::types::RowControl::Wave(crate::wave::signal::WaveSignal::Offer { wave_id: [seed; 16], nonce: [seed; 32], device: None });
            out.push(ChatMessage::control(wave, false, base + (n as i64) * (1 << 20)));
        }
        out
    }

    /// Build a vault of the owner's size: 15 friends with `OWNER_ROWS` rows each and three unread in every conversation that holds rows; returns the vault's keys.
    fn owner_sized_vault(vault_seed: [u8; 32], device_secret: [u8; 32]) -> Vec<(Contact, Vec<ChatMessage>)> {
        let storage = FlatStorage::new(crate::storage::APP, vault_seed, device_secret).unwrap();
        let mut made = Vec::new();
        for (i, n) in OWNER_ROWS.iter().enumerate() {
            let c = friend(i as u8 + 1);
            save_contact(&c, &storage).unwrap();
            let mut conv = c.conversation(&US);
            conv.messages = rows(i as u8, *n, i == 0);
            conv.unread_count = if *n > 0 { 3 } else { 0 };
            save_messages(&conv, &storage).unwrap();
            save_conversation_state(&conv, &storage).unwrap();
            made.push((c, conv.messages));
        }
        made
    }

    fn pids() -> ResumePids {
        ResumePids { handle_proof: [0x77; 32], own: Some(US), sibling: None }
    }

    /// Run the row worker to its end, `asks` queued before it starts, and collect what it posts in order.
    fn run_worker(storage: Arc<FlatStorage>, ids: Vec<ConversationId>, asks: &[ConversationId]) -> Vec<Landed> {
        let (ask_tx, ask_rx) = mpsc::channel();
        for a in asks {
            ask_tx.send(*a).unwrap();
        }
        let (tx, rx) = mpsc::channel();
        rows_worker(storage, ids, ask_rx, tx, || {});
        rx.try_iter().collect()
    }

    /// READY BEFORE THE ROWS: the vault worker's hand-off carries every contact, each conversation's unread count, and only its head (the newest visible row, thru any control rows on top), every one marked pending.
    #[test]
    fn ready_carries_contacts_and_heads_not_rows() {
        crate::storage::isolate_test_storage();
        let (vs, ds) = ([0x31; 32], [0x32; 32]);
        let made = owner_sized_vault(vs, ds);
        let storage = Arc::new(FlatStorage::new(crate::storage::APP, vs, ds).unwrap());
        let opened = open_and_load_contacts(Ok(storage), &pids(), Instant::now()).unwrap();
        assert_eq!(opened.contacts.len(), OWNER_ROWS.len());
        assert_eq!(opened.conversations.len(), OWNER_ROWS.len());
        for (c, stored) in &made {
            let conv = opened.conversations.iter().find(|v| v.id() == c.conversation(&US).id()).expect("each contact's conversation");
            assert!(conv.rows_pending && !conv.hydrated, "no conversation is hydrated before its rows land");
            assert_eq!(conv.unread_count, if stored.is_empty() { 0 } else { 3 }, "unread rides the summary");
            assert_eq!(newest_visible(&conv.messages), newest_visible(stored), "the head sorts the list as the whole table would");
            let control_head = stored.last().is_some_and(|m| m.is_control());
            assert_eq!(conv.messages.len(), if stored.is_empty() { 0 } else if control_head { 2 } else { 1 }, "the head stops at the newest visible row");
        }
    }

    /// OPENED BEFORE ITS ROWS LAND: the asked conversation goes first, its newest page lands before anything else, then the whole of it, then the rest in order; every conversation lands exactly once, whole.
    #[test]
    fn asked_conversation_lands_newest_page_first() {
        crate::storage::isolate_test_storage();
        let (vs, ds) = ([0x41; 32], [0x42; 32]);
        let made = owner_sized_vault(vs, ds);
        let storage = Arc::new(FlatStorage::new(crate::storage::APP, vs, ds).unwrap());
        let ids: Vec<ConversationId> = made.iter().map(|(c, _)| c.conversation(&US).id()).collect();
        // The 939-row conversation, opened while the first in the queue loads.
        let big = ids[2];
        let landed = run_worker(storage, ids.clone(), &[big]);
        let Landed::Page { id, rows: page } = &landed[0] else { panic!("the asked conversation's newest page lands first") };
        assert_eq!(*id, big);
        assert_eq!(page.len(), ROWS_PAGE);
        let stored = &made[2].1;
        assert_eq!(page.last().map(|m| m.timestamp), stored.last().map(|m| m.timestamp), "the page is the NEWEST rows");
        assert_eq!(page.first().map(|m| m.timestamp), Some(stored[stored.len() - ROWS_PAGE].timestamp));
        let Landed::Full { id, rows: Ok(all), .. } = &landed[1] else { panic!("then the whole asked conversation") };
        assert_eq!((*id, all.len()), (big, stored.len()));
        let fulls: Vec<ConversationId> = landed.iter().filter_map(|l| match l {
            Landed::Full { id, .. } => Some(*id),
            Landed::Page { .. } => None,
        }).collect();
        let mut rest = ids.clone();
        rest.retain(|i| *i != big);
        assert_eq!(fulls[1..], rest[..], "the rest follow in queue order, each once");
        for l in &landed {
            if let Landed::Full { id, rows: Ok(rows), .. } = l {
                let want = &made.iter().find(|(c, _)| c.conversation(&US).id() == *id).unwrap().1;
                assert_eq!(rows.len(), want.len());
                assert!(rows.windows(2).all(|w| w[0].timestamp <= w[1].timestamp), "oldest first");
            }
        }
    }

    /// THE LANDING MERGE: an opened page shown first, then rows that moved in RAM meanwhile (a star, a delivered tick to a member) and a row sent meanwhile, then the full load: every stored row, the RAM edits kept, nothing doubled, unread untouched.
    #[test]
    fn landing_keeps_ram_edits_and_unread() {
        crate::storage::isolate_test_storage();
        let (vs, ds) = ([0x51; 32], [0x52; 32]);
        let made = owner_sized_vault(vs, ds);
        let storage = Arc::new(FlatStorage::new(crate::storage::APP, vs, ds).unwrap());
        let (c, stored) = &made[1];
        let id = c.conversation(&US).id();
        let landed = run_worker(storage.clone(), vec![id], &[id]);
        let Landed::Page { rows: page, .. } = &landed[0] else { panic!("page first") };
        let Landed::Full { rows: Ok(all), .. } = &landed[1] else { panic!("then the whole") };
        let opened = open_and_load_contacts(Ok(storage), &pids(), Instant::now()).unwrap();
        let mut conv = opened.conversations.into_iter().find(|v| v.id() == id).unwrap();
        assert_eq!(conv.unread_count, 3);
        for m in page.clone() {
            conv.insert_message_sorted(m);
        }
        assert_eq!(conv.messages.len(), ROWS_PAGE, "the page and the head overlap, never double");
        let starred = conv.messages[ROWS_PAGE >> 1].timestamp;
        conv.messages[ROWS_PAGE >> 1].star_osc = 99;
        let out = conv.messages.iter().rposition(|m| m.is_outgoing).unwrap();
        let out_ts = conv.messages[out].timestamp;
        conv.messages[out].delivered_to = vec![c.handle_hash];
        let sent = ChatMessage::new_with_timestamp("sent while the rows loaded".into(), true, i64::MAX >> 1);
        conv.insert_message_sorted(sent);
        conv.adopt_loaded_rows(all.clone());
        conv.hydrated = true;
        conv.rows_pending = false;
        assert_eq!(conv.messages.len(), stored.len() + 1, "every stored row once, and the new one");
        assert!(conv.messages.windows(2).all(|w| w[0].timestamp <= w[1].timestamp));
        assert_eq!(conv.messages.iter().find(|m| m.timestamp == starred).unwrap().star_osc, 99, "a star made meanwhile survives");
        assert_eq!(conv.messages.iter().find(|m| m.timestamp == out_ts).unwrap().delivered_to, vec![c.handle_hash], "a delivery tick made meanwhile survives");
        assert_eq!(conv.messages.last().unwrap().content, "sent while the rows loaded");
        assert_eq!(conv.unread_count, 3, "unread is the summary's, before and after");
    }

    /// The head of a table whose newest rows are control rows reads thru to the visible row under them, and an empty table has no head.
    #[test]
    fn head_reads_thru_control_rows() {
        crate::storage::isolate_test_storage();
        let (vs, ds) = ([0x61; 32], [0x62; 32]);
        let made = owner_sized_vault(vs, ds);
        let storage = FlatStorage::new(crate::storage::APP, vs, ds).unwrap();
        let head = load_head_rows(made[0].0.conversation(&US).id().as_bytes(), &storage).unwrap();
        assert_eq!(head.len(), 2);
        assert!(head[1].is_control() && !head[0].is_control());
        assert!(load_head_rows(made[5].0.conversation(&US).id().as_bytes(), &storage).unwrap().is_empty());
    }

    /// THE TIMING SPLIT on the desktop, the owner's size (15 conversations, 2339 rows): launch-to-Ready's vault work before (every row on the UI thread) and after (contacts and summaries on the vault worker), and where the rows' time goes.
    /// Run: cargo test --lib rows_load::tests::resume_timing_split -- --ignored --nocapture
    #[test]
    #[ignore]
    fn resume_timing_split() {
        crate::storage::isolate_test_storage();
        let (vs, ds) = ([0x71; 32], [0x72; 32]);
        let made = owner_sized_vault(vs, ds);
        let total_rows: usize = OWNER_ROWS.iter().sum();
        let ms = |d: Duration| d.as_secs_f64() * 1000.;
        let ids: Vec<ConversationId> = made.iter().map(|(c, _)| c.conversation(&US).id()).collect();
        // BEFORE: the old resume, every conversation's state and rows before the first Ready frame.
        let before = {
            let storage = FlatStorage::new(crate::storage::APP, vs, ds).unwrap();
            // The vault's first read pays for its spine; timed apart so the contacts' share is their own.
            let t = Instant::now();
            storage.read_addr(&[0u8; 32]).unwrap();
            let first = t.elapsed();
            let t = Instant::now();
            let contacts = crate::storage::contacts::load_all_contacts(&storage);
            let t_contacts = t.elapsed();
            for c in &contacts {
                let mut conv = c.conversation(&US);
                crate::storage::contacts::load_conversation_state(&mut conv, &c.handle_hash, &storage);
                crate::storage::contacts::load_messages(&mut conv, &storage).unwrap();
            }
            (t_contacts, t.elapsed(), first)
        };
        // AFTER: what Ready waits on now.
        let storage = Arc::new(FlatStorage::new(crate::storage::APP, vs, ds).unwrap());
        let t = Instant::now();
        let opened = open_and_load_contacts(Ok(storage.clone()), &pids(), t).unwrap();
        let ready = t.elapsed();
        // The rows after Ready, on the worker.
        let t = Instant::now();
        let landed = run_worker(storage.clone(), ids.clone(), &[]);
        let rows_wall = t.elapsed();
        let mut timing = RowLoadTiming::default();
        for l in &landed {
            if let Landed::Full { timing: t, .. } = l {
                timing.add(t);
            }
        }
        // The vault read split: a raw fetch (no decrypt, no cache) of every row, a read thru kete on a cold cache (fetch + decrypt), and the same read again on a warm cache (decrypt alone).
        drop(opened);
        drop(storage);
        let storage = FlatStorage::new(crate::storage::APP, vs, ds).unwrap();
        let mut addrs: Vec<[u8; 32]> = Vec::new();
        for id in &ids {
            let mut t = RowLoadTiming::default();
            for (_, _, pk) in crate::storage::contacts::message_keys(id.as_bytes(), &storage, &mut t).unwrap() {
                addrs.push(rarangi::keys::row_addr(id.as_bytes(), &pk.addr_bytes()));
            }
        }
        let t = Instant::now();
        for a in &addrs {
            storage.read_stored(a).unwrap();
        }
        let fetch = t.elapsed();
        let t = Instant::now();
        for a in &addrs {
            storage.read_addr(a).unwrap();
        }
        let cold = t.elapsed();
        let t = Instant::now();
        for a in &addrs {
            storage.read_addr(a).unwrap();
        }
        let warm = t.elapsed();
        eprintln!("RESUME TIMING ({} rows in {} conversations, desktop):", total_rows, ids.len());
        eprintln!("  before: first vault read {:.1}ms, then contacts {:.1}ms, contacts+every row {:.1}ms before Ready", ms(before.2), ms(before.0), ms(before.1));
        eprintln!("  after:  contacts+summaries {:.1}ms before Ready", ms(ready));
        eprintln!("  rows after Ready: {} rows in {:.1}ms — catalog {:.1}ms, vault read+decrypt {:.1}ms, record decode {:.1}ms, row build {:.1}ms", timing.rows, ms(rows_wall), ms(timing.catalog), ms(timing.vault), ms(timing.decode), ms(timing.build));
        eprintln!("  vault split over {} row reads: raw fetch {:.1}ms, fetch+decrypt (cold cache) {:.1}ms, decrypt from the cache {:.1}ms", addrs.len(), ms(fetch), ms(cold), ms(warm));
        // What a first Ready frame may draw per contact with no picture: the mark rendered here (when the worker has not delivered it), the mark's AV1 decode (the worker's path), and its resample to a row's diameter.
        let t = Instant::now();
        let mark = super::super::mark_render_rgb(0x1234);
        let render = t.elapsed();
        let f32s: Vec<f32> = mark.iter().map(|&b| b as f32 * (1. / 255.)).collect();
        let av1 = crate::ui::avatar::encode_avatar_rgb_f32(&f32s).unwrap();
        let t = Instant::now();
        crate::ui::avatar::decode_avatar_av1_to_display(&av1).unwrap();
        let decode = t.elapsed();
        let mut scale = Vec::new();
        for d in [1usize << 6, 1 << 7, 3 << 6] {
            let t = Instant::now();
            crate::ui::avatar_render::update_avatar_scaled(&mark, super::super::MARK_SIDE, d);
            scale.push(format!("{}px {:.2}ms", d, ms(t.elapsed())));
        }
        eprintln!("  one mark: rendered inline {:.1}ms, AV1 decode {:.1}ms, resample {}", ms(render), ms(decode), scale.join(", "));
    }
}
