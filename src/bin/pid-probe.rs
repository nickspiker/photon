//! Ground-truth party-id probe for a fleet: derive every id a device wears in every naming system, so log archaeology never has to guess again.
//! Ceremony/punch log lines print `fp(contact.handle_proof)` (a SIBLING contact carries OUR OWN hp), CHAT lines print pid pseudonyms, the fleet page prints device names — three different labels for the same hardware. This maps them.
//! Usage: `pid-probe <handle>` — the handle never lives in this source or the repo.

use photon_messenger::types::Handle;

fn main() {
    let Some(handle) = std::env::args().nth(1) else {
        eprintln!("usage: pid-probe <handle>  |  pid-probe --hp <64-hex>  (chain-only: fold an arbitrary handle_proof's fleet)");
        std::process::exit(2);
    };
    // --hp mode: no handle, no seed — just fold the chain for a raw handle_proof (whose devices are these?).
    if handle == "--hp" {
        let hex_hp = std::env::args()
            .nth(2)
            .expect("--hp needs a 64-hex handle_proof");
        let bytes = (0..64)
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex_hp[i..i + 2], 16).expect("bad hex"))
            .collect::<Vec<u8>>();
        let hp: [u8; 32] = bytes.try_into().unwrap();
        match photon_messenger::network::fgtw::fleet::current_members(&hp) {
            Ok(members) => {
                println!("hp {}…  fleet members: {}", &hex_hp[..8], members.len());
                for m in &members {
                    println!("  device {}…", hex::encode(&m[..4]));
                }
            }
            Err(e) => println!("hp {}…  chain fetch failed: {}", &hex_hp[..8], e),
        }
        return;
    }
    let seed = photon_messenger::storage::contacts::derive_identity_seed(&handle);
    println!("identity_seed        = {}…", hex::encode(&seed[..8]));
    let hp = Handle::username_to_handle_proof(&handle);
    println!(
        "handle_proof         = {}…  (what ceremony/punch logs print for SIBLING contacts)",
        hex::encode(&hp[..8])
    );
    let idp = photon_messenger::crypto::clutch::identity_party_id(&seed);
    println!(
        "identity_party_id    = {}…  (self-contact pid)",
        hex::encode(&idp[..8])
    );
    let members =
        photon_messenger::network::fgtw::fleet::current_members(&hp).expect("fetch fleet chain");
    println!("fleet members: {}", members.len());
    for m in &members {
        let pid = photon_messenger::crypto::clutch::sibling_party_id(m);
        println!(
            "device {}…  sibling_pid {}…  device_name {:?}  pid_pseudonym {:?}",
            hex::encode(&m[..4]),
            hex::encode(&pid[..8]),
            photon_messenger::network::fgtw::fleet::device_name_default(m, &seed),
            photon_messenger::network::fgtw::fleet::keyed_pseudonym(&pid)
        );
    }
    // Each device's SEED-REGISTRY address record — the ground truth for "what will a peer actually punch at" (a LAN-only record explains a relay-tier ring from any peer off that LAN).
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    for m in &members {
        match rt.block_on(
            photon_messenger::network::fgtw::phonebook_client::resolve_device_address(m),
        ) {
            Ok(rec) => {
                use photon_messenger::network::fgtw::phonebook_client::bytes_to_ip;
                let age_s = (vsf::eagle_time_oscillations() - rec.epoch())
                    / vsf::OSCILLATIONS_PER_SECOND as i64;
                println!(
                    "  {}… registry record: wan {}:{}  lan {}  (published {}s ago)",
                    hex::encode(&m[..4]),
                    bytes_to_ip(&rec.ip()),
                    rec.port(),
                    bytes_to_ip(&rec.local_ip()),
                    age_s
                );
            }
            Err(e) => println!("  {}… registry record: {:?}", hex::encode(&m[..4]), e),
        }
    }
    // The pb_devices view — the REGISTRY-FIRST path peers actually resolve thru (per-device pb_get is only the fallback); a stale row HERE is what a peer punches at even when the per-device record above is fresh.
    match rt.block_on(photon_messenger::network::fgtw::phonebook_client::fetch_devices(&hp)) {
        Ok((_view, addresses)) => {
            use photon_messenger::network::fgtw::phonebook_client::bytes_to_ip;
            for rec in &addresses {
                let age_s = (vsf::eagle_time_oscillations() - rec.epoch())
                    / vsf::OSCILLATIONS_PER_SECOND as i64;
                println!(
                    "  pb_devices row: dev {}…  wan {}:{}  lan {}  (published {}s ago)",
                    hex::encode(&rec.device_pubkey()[..4]),
                    bytes_to_ip(&rec.ip()),
                    rec.port(),
                    bytes_to_ip(&rec.local_ip()),
                    age_s
                );
            }
            if addresses.is_empty() {
                println!("  pb_devices: no address rows");
            }
        }
        Err(e) => println!("  pb_devices: fetch failed: {}", e),
    }
    // The fan-out envelope is plaintext structure (epoch + rotator + per-device wraps; the KEYS inside the wraps stay sealed) — the ground truth for "which epoch is live and who minted it" when siblings disagree about the fleet key.
    match photon_messenger::network::fgtw::fleet::fetch_fanout(&hp) {
        Ok(Some((epoch, rotator, wraps))) => {
            println!(
                "fan-out: epoch {}  rotator {}…  {} wrap(s) for:",
                epoch,
                hex::encode(&rotator[..4]),
                wraps.len()
            );
            for w in &wraps {
                println!("  wrapped device (commit {}…)", hex::encode(&w.commit[..4]));
            }
        }
        Ok(None) => println!("fan-out: none published"),
        Err(e) => println!("fan-out: fetch failed: {}", e),
    }
}
