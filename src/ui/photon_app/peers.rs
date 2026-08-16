//! Peer records — registry converge, self peer-record publish, stalled-address resolve, and the on-disk peer store.

use super::*;

impl PhotonApp {
    /// Put OUR OWN device's peer record into the store, self-signed.
    ///
    /// Nothing did this before, and it is why phonebook gossip has never carried a single record:
    /// FGTW's `serialize_peer_list` emits no signature field, so every record parsed from the server arrives with `signature = [0u8; 64]` — and `PeerStore::merge_peer` opens by rejecting anything that fails `verify()`. The wire, the encode, the serve and the merge were all built; the mesh simply had nothing signed to carry.
    ///
    /// Only this device can produce this record: the signature is by the device key, over `handle_proof ‖ device_pubkey ‖ ip ‖ local_ip ‖ last_seen`. That is what lets a peer trust a record WITHOUT trusting the peer that relayed it — the property the whole gossip mesh needs, and the reason a record can propagate hop to hop instead of only from an authority.
    ///
    /// The published address is the REFLEXIVE one — what peers actually observe on the live UDP data socket — not fgtw.org's `cf-connecting-ip`, which reflects a TLS flow and is only right for cone NATs. That is also why the device can sign it at all: until reflexive discovery existed, a device did not know its own public address and could not commit to one.
    /// Converge our identity's PRIMARY registry onto a freshly-adopted fold (cutover Phase 4): read the stored view, plan the minimal placement-signed writes, put each. A `stale` rejection means a racing sibling landed first — success by other hands, logged and dropped.
    pub(super) fn spawn_registry_converge(&self, handle_proof: [u8; 32], fold: Vec<[u8; 32]>) {
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        let sk = ed25519_dalek::SigningKey::from_bytes(kp.secret.as_bytes());
        crate::network::http::runtime().spawn(async move {
            let view = match crate::network::fgtw::phonebook_client::fetch_devices(&handle_proof).await {
                Ok((view, _)) => view,
                // No registry yet (or an unreadable one): converge from empty — the plan re-mints everything.
                Err(_) => fgtw::phonebook::RegistryView::default(),
            };
            let plan = fgtw::phonebook::registry_plan(&sk, &handle_proof, &fold, &view, vsf::eagle_time_oscillations(), None);
            if plan.is_empty() {
                return;
            }
            let total = plan.len();
            let mut stored = 0usize;
            for rec in &plan {
                match crate::network::fgtw::phonebook_client::put_record(rec).await {
                    Ok(()) => stored += 1,
                    Err(e) => crate::logf!("PHONEBOOK: registry write skipped ({})", e),
                }
            }
            crate::logf!("PHONEBOOK: primary registry converged — {}/{} record(s) written for a {}-device fold", stored, total, fold.len());
        });
    }

    pub(super) fn publish_self_peer_record(&mut self) {
        let (Some(store), Some(kp), Some(addr), Some(hp)) = (
            self.peer_store.as_ref(),
            self.device_keypair.as_ref(),
            self.our_reflexive,
            self.our_handle_proof(),
        ) else {
            return; // pre-attest, or no reflexive echo yet — nothing honest to publish
        };

        // Never SIGN an unreachable address. A relay-injected pong reports the RELAY_ADDR sentinel, and adopting it produced a first-ever published record advertising 0.0.0.0:0 to the whole mesh -- a signed claim that we cannot be reached, which gossip would then dutifully propagate. `record()` now refuses the sentinel at the source; this is the belt-and-braces at the signing point, because a bad record here is the one thing that spreads.
        if crate::network::traverse::gather::is_bogus_addr(&addr) {
            return;
        }

        // LAN slot: the beacon-observed address FIRST (kernel truth for the interface the beacon left on), the routing-trick detection only as the fallback — the trick names the internet-routing interface, which on a phone routing over cellular is the CLAT/CGNAT one while the Wi-Fi holds the real LAN address (a record without its LAN entry left the peer probing an unreachable-without-hairpin WAN, 2026-08-11).
        let local_ip = self
            .our_lan_ip
            .or_else(crate::network::udp::get_local_ip)
            .map(std::net::IpAddr::V4);
        let mut rec = crate::network::fgtw::PeerRecord {
            handle_proof: hp,
            device_pubkey: crate::types::DevicePubkey::from_bytes(*kp.public.as_bytes()),
            ip: addr,
            local_ip,
            last_seen: vsf::eagle_time_oscillations(),
            signature: [0u8; 64],
        };
        rec.sign(&kp.secret);
        debug_assert!(rec.verify(), "a record we just signed must verify");

        store.lock().unwrap().add_peer(rec);
        self.self_record_published_for = Some(addr);
        // Same edge, second registry: a fresh reflexive publish is also the moment to re-check the primary registry against the last-adopted fold (empty plan when in sync — one cheap GET).
        if !self.registry_converged_fold.is_empty() {
            if let Some(our_hp) = self.our_handle_proof() {
                self.spawn_registry_converge(our_hp, self.registry_converged_fold.clone());
            }
        }
        crate::logf!(
            "PHONEBOOK: published our own signed record at {} — gossip can now carry it",
            addr
        );

        // ALSO publish to the seed's registry. Gossip alone cannot bootstrap: carrying a record needs a validated path, a path needs a punch, and a punch needs an address we could only have learned from a peer we cannot yet reach. The seed breaks that circle — it is the one place reachable without already knowing anyone. Fire-and-forget off-thread: this is a discovery-path nicety, and a seed that is down must never stall the UI or the local store (which is already updated above and persists regardless).
        let secret = kp.secret.clone();
        let local = local_ip; // the same beacon-first LAN value the signed record carries — the seed must never publish a WORSE address than gossip does
        crate::network::http::runtime().spawn(async move {
            match crate::network::fgtw::phonebook_client::publish_address(&secret, &hp, addr, local)
                .await
            {
                Ok(()) => crate::logf!("PHONEBOOK: address record published to the seed registry"),
                Err(e) => crate::logf!(
                    "PHONEBOOK: seed publish failed ({}) — gossip still carries it",
                    e
                ),
            }
        });
    }

    /// Ask the seed registry for the addresses of devices we cannot otherwise reach.
    ///
    /// This is what re-enters the discovery cycle. Every other address source needs an address already: a pong needs somewhere to send a ping, gossip needs a validated path, and a path needs a punch at an address we do not have. The seed is reachable without knowing anyone, so on a cold start it is the only way in — but it is asked LAST, after the gossip request to every peer we can already reach, because a peer's answer is fresher and costs the seed nothing.
    ///
    /// Results land in `device_endpoints`, which is what `gather_peer_candidates` reads to build punch candidates. They are NOT written to the contact-level `ip` slot: a resolved address is a claim we have not yet round-tripped, and the punch is what promotes it to a real path.
    pub(super) fn resolve_stalled_addresses_from_seed(&mut self) {
        if self.pb_resolve_rx.is_some() {
            return; // one in flight — a slow seed must not stack requests
        }
        // Per stalled contact: every device we know for it, fold included — `relay_device_list` alone missed the folded-but-never-contacted case (a device in the membership fold that never pinged us has no endpoint row, so it was never resolved and never punched).
        // "Stalled" = no USABLE address — a relay-only contact's ip slot holds the RELAY SENTINEL (0.0.0.0), and treating that as "has an address" locked the contact out of the one lookup that could upgrade it: never resolved → never learns the registry's WAN → punches only a foreign LAN → stuck on relay-tier forever, while a same-LAN device shows direct (field, 2026-08-16: friend amber from the desktop, green from the mac sitting on her LAN).
        // ROTATING walk, not a fixed-prefix walk: the budget bounds one pulse, but "the next pulse takes the rest" was a lie — every pulse restarted at contact zero, so a handful of permanently-offline contacts at the head ate the whole budget forever and everyone behind them was NEVER resolved (field, 2026-08-16: 'resolved 3 endpoint(s)' — the same 3 — every 15s all session, while the friend past the cutoff sat relay-tier with a perfect registry record nobody fetched). The cursor advances past what a pulse consumed, so every stalled contact gets its turn.
        let mut wanted: Vec<([u8; 32], Vec<[u8; 32]>)> = Vec::new();
        let mut budget = 16usize;
        let n = self.contacts.len();
        if n == 0 {
            return;
        }
        let start = self.pb_resolve_cursor % n;
        let mut consumed = 0usize;
        for step in 0..n {
            let c = &self.contacts[(start + step) % n];
            if !c
                .ip
                .map_or(true, |a| crate::network::traverse::gather::is_bogus_addr(&a))
            {
                continue;
            }
            if budget == 0 {
                break; // bounded per pulse — a large roster must not turn one stalled tick into a burst at the seed; the cursor hands the rest to the next pulse
            }
            let mut devs: Vec<[u8; 32]> = Vec::new();
            for dev in c
                .relay_device_list()
                .into_iter()
                .chain(c.fleet_members.iter().copied())
            {
                if !devs.contains(&dev) && budget > 0 {
                    devs.push(dev);
                    budget -= 1;
                }
            }
            if !devs.is_empty() {
                wanted.push((c.handle_proof, devs));
            }
            consumed = step + 1;
        }
        self.pb_resolve_cursor = (start + consumed) % n;
        if wanted.is_empty() {
            return;
        }

        let (tx, rx) = std::sync::mpsc::channel();
        self.pb_resolve_rx = Some(rx);
        crate::network::http::runtime().spawn(async move {
            let mut found = Vec::new();
            for (hp, devs) in wanted {
                // Registry-first: one pb_devices round trip answers the whole identity — count, chain-vouched pointers, and every published address. Absent until the contact's own fleet converges post-cutover, so the per-device path below stays as the fallback.
                let mut covered: Vec<[u8; 32]> = Vec::new();
                if let Ok((_view, addresses)) =
                    crate::network::fgtw::phonebook_client::fetch_devices(&hp).await
                {
                    for rec in addresses {
                        let dev = rec.device_pubkey();
                        if let Some(pubaddr) =
                            crate::network::fgtw::phonebook_client::record_socket_addr(&rec)
                        {
                            let lan =
                                crate::network::fgtw::phonebook_client::record_local_addr(&rec);
                            found.push((hp, dev, pubaddr, lan));
                        }
                        // A pointed device with no usable address is still ANSWERED — the registry spoke for it; asking again per-device would just repeat the same record.
                        covered.push(dev);
                    }
                }
                for dev in devs.into_iter().filter(|d| !covered.contains(d)) {
                    match crate::network::fgtw::phonebook_client::resolve_device_address(&dev).await
                    {
                        Ok(rec) => {
                            if let Some(pubaddr) =
                                crate::network::fgtw::phonebook_client::record_socket_addr(&rec)
                            {
                                let lan =
                                    crate::network::fgtw::phonebook_client::record_local_addr(&rec);
                                found.push((hp, dev, pubaddr, lan));
                            }
                        }
                        // Absence is the normal case for a device that has not published yet; only a genuine rejection is worth a line.
                        Err(crate::network::fgtw::phonebook_client::PhonebookError::NotFound) => {}
                        Err(e) => crate::logf!("PHONEBOOK: seed lookup failed ({})", e),
                    }
                }
            }
            let _ = tx.send(found);
        });
    }

    /// Apply resolved seed addresses to the per-device endpoints, and ping so the punch fires at once.
    pub(super) fn drain_pb_resolve(&mut self) -> bool {
        let Some(rx) = self.pb_resolve_rx.as_ref() else {
            return false;
        };
        let found = match rx.try_recv() {
            Ok(f) => f,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pb_resolve_rx = None;
                return false;
            }
        };
        self.pb_resolve_rx = None;
        if found.is_empty() {
            return false;
        }
        let mut learned = 0usize;
        for (hp, dev, pubaddr, lan) in found {
            // Never adopt the relay sentinel or an unspecified address as an endpoint — it validates locally and then poisons every send that keys off `validated_path`.
            if crate::network::traverse::gather::is_bogus_addr(&pubaddr) {
                continue;
            }
            for contact in self.contacts.iter_mut() {
                // A FRIEND adopts every device the registry vouches for its identity — the registry is fresher than the contact, and gating on the devices we already knew rejected exactly the record that heals a dead pin (the contact pinned a retired pre-wipe device, its relay sends went to a key nobody polls, and the live fleet's endpoints were resolved and then thrown away — live pair, 2026-08-03). The endpoint upsert extends the relay fan-out to the vouched device; message trust is unchanged, every parser still signature-gates. SIBLING rows stay device-scoped: each row is one device, and identity-wide adoption would smear every sibling's endpoint onto every row.
                let ours = if contact.is_sibling {
                    contact.relay_device_list().contains(&dev)
                } else {
                    contact.handle_proof == hp
                };
                if !ours {
                    continue;
                }
                let ep = contact.endpoint_mut(&dev);
                ep.public = Some(pubaddr);
                if let Some(l) = lan.filter(|a| !crate::network::traverse::gather::is_bogus_addr(a))
                {
                    ep.lan = Some(l);
                }
                learned += 1;
            }
        }
        if learned > 0 {
            crate::logf!(
                "PHONEBOOK: resolved {} device endpoint(s) from the seed registry — punching",
                learned
            );
            self.ping_contacts();
            return true;
        }
        false
    }

    /// Write the phonebook to the vault so it survives a restart.
    ///
    /// Write the peer store to the vault so "peers first, seed last" survives a relaunch. Fires on the own-address edge (the reflexive publish site) and on the observed-growth edge in the stalled-contact harvest — gossip merges land on the checker thread, so a UI tick observes and persists them.
    ///
    /// Only self-signed rows persist (see `to_vsf_bytes`) — an unsigned FGTW row can never be gossiped or trusted by a peer, so carrying it across a restart would just grow the file.
    pub(super) fn persist_peer_store(&self) {
        let (Some(store), Some(storage), Some(session)) = (
            self.peer_store.as_ref(),
            self.storage.as_ref(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        let bytes = match store.lock().unwrap().to_vsf_bytes(kp) {
            Ok(b) => b,
            Err(e) => {
                crate::logf!("PHONEBOOK: encode failed, not persisting: {}", e);
                return;
            }
        };
        let addr = crate::storage::vault_key("peers", &session.vault_seed);
        match storage.write_addr(&addr, &bytes) {
            Ok(()) => crate::logf!("PHONEBOOK: persisted ({} bytes)", bytes.len()),
            Err(e) => crate::logf!("PHONEBOOK: persist failed: {}", e),
        }
    }

    /// Load the persisted phonebook into the live store, merging rather than replacing — a record learned this session (fresher `last_seen`) must win over a stale one off disk, which is what `add_peer`'s per-device upsert already does.
    pub(super) fn load_peer_store(&mut self) {
        let (Some(store), Some(storage), Some(session)) = (
            self.peer_store.as_ref(),
            self.storage.as_ref(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let addr = crate::storage::vault_key("peers", &session.vault_seed);
        let Ok(Some(bytes)) = storage.read_addr(&addr) else {
            return; // first run, or nothing persisted yet
        };
        match crate::network::fgtw::PeerStore::from_vsf_bytes(&bytes) {
            Ok(loaded) => {
                let mut live = store.lock().unwrap();
                let mut n = 0usize;
                for rec in loaded.get_all_peers() {
                    live.add_peer(rec);
                    n += 1;
                }
                crate::logf!("PHONEBOOK: loaded {} signed record(s) from the vault", n);
            }
            // A vault a disk error touched must not inject peers — the verified read refused it.
            Err(e) => crate::logf!(
                "PHONEBOOK: persisted phonebook unreadable, starting fresh: {}",
                e
            ),
        }
    }
}
