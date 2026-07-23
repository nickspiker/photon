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
        let hex_hp = std::env::args().nth(2).expect("--hp needs a 64-hex handle_proof");
        let bytes = (0..64).step_by(2).map(|i| u8::from_str_radix(&hex_hp[i..i + 2], 16).expect("bad hex")).collect::<Vec<u8>>();
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
    println!("handle_proof         = {}…  (what ceremony/punch logs print for SIBLING contacts)", hex::encode(&hp[..8]));
    let idp = photon_messenger::crypto::clutch::identity_party_id(&seed);
    println!("identity_party_id    = {}…  (self-contact pid)", hex::encode(&idp[..8]));
    let members = photon_messenger::network::fgtw::fleet::current_members(&hp).expect("fetch fleet chain");
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
}
