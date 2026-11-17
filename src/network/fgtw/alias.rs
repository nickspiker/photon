//! Photon's alias, where Photon carries FGTW (the desktop and Android; ferros's kernel hands it in): ferros docs/PHOTON.md "Photon's own key: the migration", steps 1 and 2.
//! The person key is found in the fleet's space or, the first time anywhere, minted (fgtw::person::settle, the one decision ferros's identity service makes too); Photon's alias derives from it with the same constants the kernel uses (fgtw::alias::PHOTON_*), so every device of the person's holds the same alias.
//! This device then signs the binding ("this alias speaks for this person in Photon") as a fleet member with every egg, and makes sure its own key is in the alias's device list at fgtw.org, so the person's other Photons (a ferros one among them) take it as theirs.
//! Nothing moves on disk here: PhotonKeys carries the identity seed as `legacy_seed`, so every vault address stays where it was until this device speaks the alias.
//! Blocking: run off the UI thread.

use super::fleet::{held_chain, PhotonTransport, SigningBundle};
use crate::crypto::photon_keys::PhotonKeys;
use fgtw::alias_devices::DeviceList;
use fgtw::pq::FleetSigner;

/// What one settle of the alias came to.
pub struct AliasReady {
    pub keys: PhotonKeys,
    /// The alias's device list as fgtw.org holds it, with this device in it, beside its binding.
    pub list: Option<(Vec<u8>, DeviceList)>,
}

/// The vault entry holding the fleet's space document as this device last merged it, so the person key (with its mint stamp) is at hand offline and `held` is never a guess.
fn space_addr(identity_seed: &[u8; 32]) -> [u8; 32] {
    crate::storage::vault_key("space", identity_seed)
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
/// `fleet_key` opens the fleet's space; `signer` is this device's full bundle (every egg the chain says it holds).
/// Err when the person key cannot be had: fgtw.org did not answer and no key is held (never minted on a pull that did not complete), the key is sealed under a fleet key this device does not hold, or the chain is not held.
pub fn settle(identity_seed: &[u8; 32], handle_proof: &[u8; 32], fleet_key: &[u8; 32], signer: &SigningBundle, device_secret: &[u8; 32]) -> Result<AliasReady, String> {
    let vault = crate::storage::device_vault().ok_or("no vault")?;
    let me = signer.keypair().public.to_bytes();
    let now = crate::network::time_base::now_osc();
    let mut space = match vault.read_addr(&space_addr(identity_seed)).map_err(|e| format!("space read: {e}"))? {
        Some(doc) => fgtw::space::Space::from_doc(&doc)?,
        None => fgtw::space::Space::new(),
    };
    // What this device held before the fleet spoke: its key and mint stamp, so a fleet-key change re-seals rather than waits, and a space without the fact gets it back.
    let held = match fgtw::person::find(&space, handle_proof, fleet_key) {
        fgtw::person::Found::Key { key, minted, .. } => Some((key, minted)),
        _ => None,
    };
    let pulled = fgtw::client::pull_space(&PhotonTransport, handle_proof);
    let heard = match &pulled {
        Ok(Some(sealed)) => {
            let doc = fgtw::space::open(sealed, handle_proof, fleet_key)?;
            space.merge(fgtw::space::Space::from_doc(&doc)?);
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
            fgtw::person::Settled::Resealed { .. } => held.map(|(k, _)| k).ok_or("resealed a key not held")?,
            fgtw::person::Settled::WaitingElsewhere { .. } => return Err("the person key is sealed under a fleet key this device does not hold; waiting for a device that does".into()),
            fgtw::person::Settled::NoEgg => return Err("the person key could not be minted: too few eggs".into()),
        };
        if matches!(s.what, fgtw::person::Settled::Minted { .. }) {
            crate::log("ALIAS: no person key in the fleet's space; minted one here");
        }
        if s.changed {
            let sealed = fgtw::space::seal(&space.to_doc(now)?, handle_proof, fleet_key)?;
            if let Err(e) = fgtw::client::push_space(&PhotonTransport, handle_proof, signer, &sealed) {
                crate::logf!("ALIAS: the space's push to fgtw.org failed ({}); kept here, sent again at the next settle", e);
            }
        }
        key
    } else {
        held.map(|(k, _)| k).ok_or("no person key held, and fgtw.org did not answer")?
    };
    // Every change hits the vault: the merged space is this device's copy from now on.
    vault.write_addr(&space_addr(identity_seed), &space.to_doc(now)?).map_err(|e| format!("space write: {e}"))?;

    let alias = fgtw::alias::seed(&person_key, &fgtw::alias::PHOTON_DEVELOPER, &fgtw::alias::PHOTON_OPTION);
    let alias_signer = fgtw::alias::bundle(&alias);
    let chain = held_chain(handle_proof).ok_or("our fleet chain is not held")?;
    let mask = chain.declared_mask(&me);
    let binding = fgtw::binding::Binding::sign(signer, mask, handle_proof, &fgtw::alias::PHOTON_DEVELOPER, &fgtw::alias::PHOTON_OPTION, alias_signer.public(), now);
    binding.verify(&chain).map_err(|e| format!("our own binding does not hold against our chain ({e:?})"))?;
    let binding = binding.to_vsf_bytes()?;
    let identity_pub = crate::crypto::clutch::identity_party_id(identity_seed);
    let keys = PhotonKeys::new(alias, *handle_proof, identity_pub, binding, Some(*identity_seed));
    let list = join_list(&keys, &me);
    Ok(AliasReady { keys, list })
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

/// This device in the alias's device list, on every platform (ferros's Photon with the alias and binding it was handed, the desktop and Android with their own): fetch the list, check it against the person's chain, add this device when missing, publish. A stale answer means another device published first, so once more on top of theirs.
/// The list as it stands with this device in it, beside its binding; None when fgtw.org could not be reached or the list could not be made.
pub fn join_list(keys: &PhotonKeys, device: &[u8; 32]) -> Option<(Vec<u8>, DeviceList)> {
    let Some(chain) = our_chain(&keys.handle_proof) else {
        crate::log("ALIAS: no chain of ours held or fetched; the device list is not joined");
        return None;
    };
    // Derived only when a list must be signed: the bundle is a Falcon and a SPHINCS+ keygen (about 380 ms on ferros), and a device already listed signs nothing.
    let mut alias_signer: Option<fgtw::pq::SigningBundle> = None;
    let now = crate::network::time_base::now_osc();
    for _ in 0..2 {
        let held_list = match fgtw::client::fetch_alias_devices(&PhotonTransport, &keys.handle_proof) {
            Ok(Some((b, l))) => match fgtw::alias_devices::check(&b, &l, &chain) {
                Ok(_) => Some(l),
                Err(e) => {
                    crate::logf!("ALIAS: fgtw.org's device list does not check ({}); starting from ours", e);
                    None
                }
            },
            Ok(None) => None,
            Err(e) => {
                crate::logf!("ALIAS: the device list did not come ({})", e);
                return None;
            }
        };
        if let Some(l) = held_list.as_ref().filter(|l| l.contains(device)) {
            return Some((keys.binding.clone(), l.clone()));
        }
        let signer = alias_signer.get_or_insert_with(|| fgtw::alias::bundle(&keys.alias));
        match DeviceList::with_device(held_list.as_ref(), signer, &keys.handle_proof, *device, now) {
            None => return held_list.map(|l| (keys.binding.clone(), l)),
            Some(next) => match fgtw::client::publish_alias_devices(&PhotonTransport, &keys.binding, &next, now) {
                Ok(()) => {
                    crate::logf!("ALIAS: this device added to the alias's device list ({} device(s))", next.devices.len());
                    return Some((keys.binding.clone(), next));
                }
                Err(e) if e == "alias_devices: stale" => continue,
                Err(e) => {
                    crate::logf!("ALIAS: the device list publish failed ({})", e);
                    return None;
                }
            },
        }
    }
    crate::log("ALIAS: the device list stayed stale after two publishes; joined at the next settle");
    None
}
