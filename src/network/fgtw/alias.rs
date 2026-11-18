//! Photon's alias, where Photon carries FGTW (the desktop and Android; ferros's kernel hands it in): ferros docs/PHOTON.md "Photon's own key: the migration", steps 1 and 2.
//! The person key is found in the fleet's space or, the first time anywhere, minted (fgtw::person::settle, the one decision ferros's identity service makes too); Photon's alias derives from it with the same constants the kernel uses (fgtw::alias::PHOTON_*), so every device of the person's holds the same alias.
//! This device then signs the binding ("this alias speaks for this person in Photon") as a fleet member with every egg, and makes sure its own key is in the alias's device list at fgtw.org, so the person's other Photons (a ferros one among them) take it as theirs.
//! Nothing moves on disk here: PhotonKeys carries the identity seed as `legacy_seed`, so every vault address stays where it was until this device speaks the alias.
//! Blocking: run off the UI thread.

use super::fleet::{held_chain, PhotonTransport, SigningBundle};
use crate::crypto::photon_keys::PhotonKeys;
use fgtw::alias_devices::{DeviceList, SignOut};
use fgtw::pq::FleetSigner;

/// What one settle of the alias came to.
pub struct AliasReady {
    pub keys: PhotonKeys,
    /// The alias's device list as the join left it, with what the chain says is locked.
    pub joined: Joined,
}

/// The vault entry holding the fleet's space document as this device last merged it, so the person key (with its mint stamp) is at hand offline and `held` is never a guess.
fn space_addr(identity_seed: &[u8; 32]) -> [u8; 32] {
    crate::storage::vault_key("space", identity_seed)
}

/// THE KEYS A ROTATION LEFT BEHIND (2026-11-17: the desktop's shrink heal minted a new fleet key and overwrote the old one, and the person key, sealed under the old, could be opened by no device; ferros docs/IDENTITY-SPACE.md "A rotation that left the person key behind").
/// Every place that replaces this device's fleet key keeps the one it replaces here (concatenated keys, newest last), until the person key is sealed under the current key at fgtw.org; the settle reads the person key and the space under any of them.
fn prior_addr(vault_seed: &[u8; 32]) -> [u8; 32] {
    crate::storage::vault_key("fleet_key.prior", vault_seed)
}

/// A fleet-key replacement since the last settle: the UI thread takes it and settles again, so the person key moves to the new key while the old is still at hand.
pub static FLEET_KEY_MOVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Keep `old` among the prior fleet keys when `new` replaces it (call it before the vault's fleet key is overwritten), and ask for a settle.
pub fn keep_prior_fleet_key(storage: &crate::storage::FlatStorage, vault_seed: &[u8; 32], old: &[u8; 32], new: &[u8; 32]) {
    if old == new {
        return;
    }
    let mut priors = prior_fleet_keys(storage, vault_seed);
    if !priors.contains(old) {
        priors.push(*old);
        let bytes: Vec<u8> = priors.iter().flatten().copied().collect();
        crate::log_err!(storage.write_addr(&prior_addr(vault_seed), &bytes), "keep the fleet key a rotation replaced");
        crate::logf!("ALIAS: the fleet key moved; the one it replaced is kept until the person key is sealed under the new one ({} kept)", priors.len());
    }
    FLEET_KEY_MOVED.store(true, std::sync::atomic::Ordering::Release);
}

/// Write `new` as this device's fleet key, keeping the one it replaces among the priors: every place a fleet key is minted, synced or recovered writes thru here.
pub fn replace_fleet_key(storage: &crate::storage::FlatStorage, vault_seed: &[u8; 32], new: &[u8; 32]) -> Result<(), crate::storage::StorageError> {
    let addr = crate::storage::vault_key("fleet_key", vault_seed);
    if let Some(old) = storage.read_addr(&addr).ok().flatten().and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok()) {
        keep_prior_fleet_key(storage, vault_seed, &old, new);
    }
    storage.write_addr(&addr, new)
}

/// The fleet keys a rotation left behind on this device, oldest first.
pub fn prior_fleet_keys(storage: &crate::storage::FlatStorage, vault_seed: &[u8; 32]) -> Vec<[u8; 32]> {
    storage
        .read_addr(&prior_addr(vault_seed))
        .ok()
        .flatten()
        .map(|b| b.chunks_exact(32).filter_map(|c| <[u8; 32]>::try_from(c).ok()).collect())
        .unwrap_or_default()
}

/// The independent sources a first person key is minted from (fgtw::person::mint takes at least two): the OS's generator, the timing of this thread's own work, and this device's secret with the clock.
fn eggs(device_secret: &[u8; 32]) -> Vec<Vec<u8>> {
    use rand::RngCore;
    let mut os = vec![0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut os);
    let mut jitter = blake3::Hasher::new_derive_key("photon 2026-11-14 person key egg: timing");
    let mut prev = std::time::Instant::now();
    for _ in 0..(1 << 10) {
        let now = std::time::Instant::now();
        jitter.update(&(now - prev).as_nanos().to_le_bytes());
        prev = now;
    }
    let mut device = blake3::Hasher::new_derive_key("photon 2026-11-14 person key egg: device");
    device.update(device_secret);
    device.update(&crate::network::time_base::now_osc().to_le_bytes());
    vec![os, jitter.finalize().as_bytes().to_vec(), device.finalize().as_bytes().to_vec()]
}

/// Settle the person key and the alias, sign the binding, and put this device in the alias's device list.
/// `fleet_key` opens the fleet's space; `vault_seed` addresses the fleet keys a rotation left behind on this device ([`keep_prior_fleet_key`]), under which the person key and the space are found too, and the person key is sealed again under `fleet_key`; `signer` is this device's full bundle (every egg the chain says it holds).
/// Err when the person key cannot be had: fgtw.org did not answer and no key is held (never minted on a pull that did not complete), the key is sealed under a fleet key this device does not hold, or the chain is not held.
pub fn settle(identity_seed: &[u8; 32], vault_seed: &[u8; 32], handle_proof: &[u8; 32], fleet_key: &[u8; 32], signer: &SigningBundle, device_secret: &[u8; 32], kept: &Kept) -> Result<AliasReady, String> {
    let vault = crate::storage::device_vault().ok_or("no vault")?;
    let me = signer.keypair().public.to_bytes();
    let now = crate::network::time_base::now_osc();
    let priors = prior_fleet_keys(&vault, vault_seed);
    // The current key first, then every key a rotation left here, newest first.
    let keys: Vec<&[u8; 32]> = std::iter::once(fleet_key).chain(priors.iter().rev()).collect();
    let mut space = match vault.read_addr(&space_addr(identity_seed)).map_err(|e| format!("space read: {e}"))? {
        Some(doc) => fgtw::space::Space::from_doc(&doc)?,
        None => fgtw::space::Space::new(),
    };
    // What this device held before the fleet spoke: its key and mint stamp, under the current fleet key or one a rotation left here, so a fleet-key change re-seals rather than waits, and a space without the fact gets it back.
    let held = keys.iter().find_map(|k| match fgtw::person::find(&space, handle_proof, k) {
        fgtw::person::Found::Key { key, minted, .. } => Some((key, minted)),
        _ => None,
    });
    let pulled = fgtw::client::pull_space(&PhotonTransport, handle_proof);
    // Whether fgtw.org's own copy already holds the person key under the current fleet key: then the keys left behind have done their work.
    let mut remote_current = false;
    let heard = match &pulled {
        Ok(Some(sealed)) => {
            let doc = keys.iter().find_map(|k| fgtw::space::open(sealed, handle_proof, k).ok()).ok_or("fgtw.org's space opens under no fleet key this device holds")?;
            let remote = fgtw::space::Space::from_doc(&doc)?;
            remote_current = matches!(fgtw::person::find(&remote, handle_proof, fleet_key), fgtw::person::Found::Key { .. });
            space.merge(remote);
            true
        }
        Ok(None) => true,
        Err(e) => {
            crate::logf!("ALIAS: fgtw.org's space did not answer ({}); the held person key only, nothing minted", e);
            false
        }
    };
    let person_key = if heard {
        let s = fgtw::person::settle(&mut space, handle_proof, fleet_key, held.as_ref().map(|(k, m)| (k, *m)), || eggs(device_secret), now, &me)?;
        let key = match s.what {
            fgtw::person::Settled::Opened { key, .. } | fgtw::person::Settled::Minted { key, .. } => key,
            fgtw::person::Settled::Resealed { .. } => {
                crate::log("ALIAS: the person key was sealed under a fleet key a rotation left behind; sealed again under the current one");
                held.map(|(k, _)| k).ok_or("resealed a key not held")?
            }
            fgtw::person::Settled::WaitingElsewhere { id, .. } => {
                // On the device only: which person fact, against how many keys, so a stranded fact shows in this device's log (ferros docs/IDENTITY-SPACE.md).
                crate::logf!("ALIAS: person fact {}… opens under none of this device's {} fleet key(s) (the current one and {} a rotation left behind)", hex::encode(&id[..4]), keys.len(), priors.len());
                return Err("the person key is sealed under a fleet key this device does not hold; waiting for a device that does".into());
            }
            fgtw::person::Settled::NoEgg => return Err("the person key could not be minted: too few eggs".into()),
        };
        if matches!(s.what, fgtw::person::Settled::Minted { .. }) {
            crate::log("ALIAS: no person key in the fleet's space; minted one here");
        }
        // Sent when the space changed, and whenever fgtw.org's copy does not yet hold the person key under the current key (a push that failed after an earlier re-seal here is sent again).
        if s.changed || !remote_current {
            let sealed = fgtw::space::seal(&space.to_doc(now)?, handle_proof, fleet_key)?;
            match fgtw::client::push_space(&PhotonTransport, handle_proof, signer, &sealed) {
                Ok(()) => remote_current = true,
                Err(e) => crate::logf!("ALIAS: the space's push to fgtw.org failed ({}); kept here, sent again at the next settle", e),
            }
        }
        key
    } else {
        held.map(|(k, _)| k).ok_or("no person key held, and fgtw.org did not answer")?
    };
    // Every change hits the vault: the merged space is this device's copy from now on.
    vault.write_addr(&space_addr(identity_seed), &space.to_doc(now)?).map_err(|e| format!("space write: {e}"))?;
    // fgtw.org holds the person key under the current key: the keys left behind are let go.
    if remote_current && !priors.is_empty() {
        crate::log_err!(vault.write_addr(&prior_addr(vault_seed), &[]), "let go of the fleet keys a rotation left behind");
        crate::logf!("ALIAS: the person key is sealed under the current fleet key at fgtw.org; {} key(s) a rotation left behind let go", priors.len());
    }

    let alias = fgtw::alias::seed(&person_key, &fgtw::alias::PHOTON_DEVELOPER, &fgtw::alias::PHOTON_OPTION);
    let alias_signer = fgtw::alias::bundle(&alias);
    let chain = held_chain(handle_proof).ok_or("our fleet chain is not held")?;
    let mask = chain.declared_mask(&me);
    let binding = fgtw::binding::Binding::sign(signer, mask, handle_proof, &fgtw::alias::PHOTON_DEVELOPER, &fgtw::alias::PHOTON_OPTION, alias_signer.public(), now);
    binding.verify(&chain).map_err(|e| format!("our own binding does not hold against our chain ({e:?})"))?;
    let binding = binding.to_vsf_bytes()?;
    let identity_pub = crate::crypto::clutch::identity_party_id(identity_seed);
    let keys = PhotonKeys::new(alias, *handle_proof, identity_pub, binding, Some(*identity_seed));
    let joined = join_list(&keys, &me, kept);
    Ok(AliasReady { keys, joined })
}

/// The person's fleet chain, as held, else as fgtw.org has it (ferros's Photon holds none: it is never a member), folded before it is trusted.
fn our_chain(handle_proof: &[u8; 32]) -> Option<fgtw::fleet::MembershipBlob> {
    if let Some(c) = held_chain(handle_proof) {
        return Some(c);
    }
    match fgtw::client::fetch(&PhotonTransport, handle_proof) {
        Ok(Some(c)) if c.fold().is_ok() && c.handle_proof() == Some(*handle_proof) => Some(c),
        Ok(Some(_)) => {
            crate::log("ALIAS: fgtw.org's chain for us does not fold, or names another identity");
            None
        }
        Ok(None) => {
            crate::log("ALIAS: fgtw.org holds no chain for us");
            None
        }
        Err(e) => {
            crate::logf!("ALIAS: our fleet chain did not come ({})", e);
            None
        }
    }
}

/// The list this device last kept, with the sign-outs it carried: what a join measures fgtw.org's list against, so a device is never dropped from the list it acts on without a backing.
#[derive(Clone, Default)]
pub struct Kept {
    pub list: Option<DeviceList>,
    pub signouts: Vec<SignOut>,
}

/// What a join of the alias's device list came to.
#[derive(Default)]
pub struct Joined {
    /// The list as it stands with this device in it, beside its binding; None when fgtw.org could not be reached, the list could not be made, or this device is locked out.
    /// Never one that leaves out a device the kept list named without that device's own sign-out or the chain backing it (fgtw alias_devices::unbacked_drops).
    pub list: Option<(Vec<u8>, DeviceList)>,
    /// The sign-outs that ride with the list: the records backing each device it leaves out.
    pub signouts: Vec<SignOut>,
    /// The devices outside the membership our chain has locked out (OpKind::LockListed), as the chain the join checked against reads them.
    pub listed_locked: Vec<[u8; 32]>,
    /// The chain has locked THIS device out: it joins nothing, and Photon on it signs out (the lock is the person's word that it is lost).
    pub self_locked: bool,
}

/// fgtw.org's slot, its list checked against `chain` for its signature (not its locks: this is the holder that prunes them), with the sign-outs it carries: Ok(None) when fgtw.org holds none or one that is not the person's, Err when fgtw.org did not answer.
fn fetch_signed_slot(keys: &PhotonKeys, chain: &fgtw::fleet::MembershipBlob) -> Result<Option<(DeviceList, Vec<SignOut>)>, String> {
    match fgtw::client::fetch_alias_slot(&PhotonTransport, &keys.handle_proof) {
        Ok(Some((b, l, s))) => match fgtw::alias_devices::check_signed(&b, &l, chain) {
            Ok(_) => Ok(Some((l, s))),
            Err(e) => {
                crate::logf!("ALIAS: fgtw.org's device list does not check ({}); starting from the one kept here", e);
                Ok(None)
            }
        },
        Ok(None) => Ok(None),
        Err(e) => Err(format!("the device list did not come ({e})")),
    }
}

/// The sign-outs known here and at fgtw.org, together.
fn all_signouts(a: &[SignOut], b: &[SignOut]) -> Vec<SignOut> {
    let mut out = a.to_vec();
    for s in b {
        if !out.contains(s) {
            out.push(s.clone());
        }
    }
    out
}

/// This device in the alias's device list, on every platform (ferros's Photon with the alias and binding it was handed, the desktop and Android with their own): fetch the slot, check it against the person's chain, add this device when missing, leave out every device the chain has locked out or seen leave (fgtw alias_devices::chain_drops), and put back every device the kept list named that fgtw.org's list leaves out with no backing, then publish. A stale answer means another device published first, so once more on top of theirs.
/// THE OWNER'S RULE (2026-11-18, "the other needs the device present to sign itself out"): a list that merely omits a device is not a removal, since every Photon of the person holds the alias; the device stays in the list this device acts on and is published back, until its own signed sign-out or the chain backs its leaving.
/// A device the chain has locked out joins nothing: a stolen one that signs a list naming itself is refused by every reader (alias_devices::check), and an honest one finds out here.
pub fn join_list(keys: &PhotonKeys, device: &[u8; 32], kept: &Kept) -> Joined {
    let Some(chain) = our_chain(&keys.handle_proof) else {
        crate::log("ALIAS: no chain of ours held or fetched; the device list is not joined");
        return Joined::default();
    };
    let listed_locked = chain.listed_locked().unwrap_or_default();
    let drops = fgtw::alias_devices::chain_drops(&chain);
    if chain.all_locked().unwrap_or_default().contains(device) {
        crate::log("ALIAS: our chain has locked this device out; it joins no list");
        return Joined { listed_locked, self_locked: true, ..Joined::default() };
    }
    let failed = |listed_locked: Vec<[u8; 32]>| Joined { listed_locked, ..Joined::default() };
    // Derived only when a list must be signed: the bundle is a Falcon and a SPHINCS+ keygen (about 380 ms on ferros), and a device already listed signs nothing.
    let mut alias_signer: Option<fgtw::pq::SigningBundle> = None;
    let now = crate::network::time_base::now_osc();
    for _ in 0..2 {
        let (fetched, fetched_signouts) = match fetch_signed_slot(keys, &chain) {
            Ok(Some((l, s))) => (Some(l), s),
            Ok(None) => (None, Vec::new()),
            Err(e) => {
                crate::logf!("ALIAS: {}", e);
                return failed(listed_locked);
            }
        };
        let signouts = all_signouts(&fetched_signouts, &kept.signouts);
        // Put back what fgtw.org's list leaves out with nothing backing it.
        let restore = match (kept.list.as_ref(), fetched.as_ref()) {
            (Some(k), Some(f)) => fgtw::alias_devices::unbacked_drops(k, f, &signouts, &chain),
            _ => Vec::new(),
        };
        let base = fetched.clone().or_else(|| kept.list.clone());
        let names_drop = base.as_ref().is_some_and(|l| l.devices.iter().any(|d| drops.contains(d)));
        if let Some(l) = fetched.as_ref().filter(|l| l.contains(device) && !names_drop && restore.is_empty()) {
            return Joined { list: Some((keys.binding.clone(), l.clone())), signouts: fgtw::alias_devices::carried(&signouts, l), listed_locked, self_locked: false };
        }
        let signer = alias_signer.get_or_insert_with(|| fgtw::alias::bundle(&keys.alias));
        let mut adds = vec![*device];
        adds.extend(restore.iter().copied());
        // With nothing at fgtw.org to build on, the kept list is signed again at a new revision, so the slot is never left empty by a device that holds one.
        let next = match DeviceList::revised(base.as_ref(), signer, &keys.handle_proof, &adds, &drops, now) {
            Some(next) => next,
            None if fetched.is_none() => match base.as_ref() {
                Some(b) => DeviceList::sign(signer, &keys.handle_proof, now.max(b.revision + 1), b.devices.clone(), b.complete),
                None => return failed(listed_locked),
            },
            None => return Joined { list: fetched.map(|l| (keys.binding.clone(), l)), signouts, listed_locked, self_locked: false },
        };
        let carried = fgtw::alias_devices::carried(&signouts, &next);
        match fgtw::client::publish_alias_slot(&PhotonTransport, &keys.binding, &next, &carried, now) {
            Ok(()) => {
                let left = base.as_ref().map_or(0, |l| l.devices.iter().filter(|d| drops.contains(d)).count());
                crate::logf!("ALIAS: the device list published with this device in it ({} device(s), {} left out as locked or gone, {} put back with no sign out behind their leaving)", next.devices.len(), left, restore.len());
                return Joined { list: Some((keys.binding.clone(), next)), signouts: carried, listed_locked, self_locked: false };
            }
            Err(e) if e == "alias_devices: stale" => continue,
            Err(e) => {
                crate::logf!("ALIAS: the device list publish failed ({})", e);
                return failed(listed_locked);
            }
        }
    }
    crate::log("ALIAS: the device list stayed stale after two publishes; joined at the next settle");
    failed(listed_locked)
}

/// SIGN OUT (the owner 2026-11-18: "the other needs the device present to sign itself out, thus asserting it wiped its keys"): this device signs its own sign-out with its own key (fgtw alias_devices::SignOut, over the person, its key and the revision it leaves at) and publishes the alias's list without itself, the record riding beside it, before it discards Photon's state here.
/// No re-key follows: the device asserts it wiped. Its siblings drop its row when they next read the list, and only because the record backs its leaving.
/// Ok when the list no longer names this device (already gone included); Err when fgtw.org could not be reached or the list could not be checked, so the person can try again and nothing is discarded yet.
pub fn leave_list(keys: &PhotonKeys, device_key: &impl fgtw::pq::FleetSigner, kept: &Kept) -> Result<(), String> {
    let device = device_key.keypair().public.to_bytes();
    let chain = our_chain(&keys.handle_proof).ok_or("no chain of ours held or fetched, so the list cannot be checked")?;
    let drops = fgtw::alias_devices::chain_drops(&chain);
    let now = crate::network::time_base::now_osc();
    let signer = fgtw::alias::bundle(&keys.alias);
    for _ in 0..2 {
        let Some((held, fetched_signouts)) = fetch_signed_slot(keys, &chain)? else {
            return Ok(());
        };
        let mut out = drops.clone();
        out.push(device);
        let Some(next) = DeviceList::revised(Some(&held), &signer, &keys.handle_proof, &[], &out, now) else {
            return Ok(());
        };
        let mut signouts = all_signouts(&fetched_signouts, &kept.signouts);
        signouts.push(SignOut::sign(device_key, &keys.handle_proof, next.revision));
        let carried = fgtw::alias_devices::carried(&signouts, &next);
        match fgtw::client::publish_alias_slot(&PhotonTransport, &keys.binding, &next, &carried, now) {
            Ok(()) => {
                crate::logf!("ALIAS: signed out: the device list published without this device, its own sign out beside it ({} device(s) left)", next.devices.len());
                return Ok(());
            }
            Err(e) if e == "alias_devices: stale" => continue,
            Err(e) => return Err(format!("the device list publish failed ({e})")),
        }
    }
    Err("the device list stayed stale after two publishes".into())
}
