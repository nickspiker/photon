//! Wi-Fi Direct bearer — the off-grid tier (docs/offgrid.md).
//!
//! Tier ladder, all ciphertext so ordering is pure cost/availability: LAN → WAN (punched) → Wi-Fi Direct → relay last resort.
//! This module is deliberately NOT a transport: once a P2P group forms, both sides hold real IPs on the p2p interface and frames ride the EXISTING wildcard-bound UDP socket — no new select! arm, no sentinel, no inject channel.
//! What lives here: the credential record friends pre-provision to each other over the normal sealed channel, the rotating discovery tokens that let friends recognise each other in DNS-SD service frames without leaking identity, the deterministic group-owner tie-break, the edge-driven bearer state machine, and the platform trait the Android/Linux radio bridges implement.
//! Hard constraint (user mandate): bringing up a group must NEVER disconnect the device's infrastructure WiFi — the platform layers rely on STA+P2P concurrency and never touch the infra connection.

use std::sync::atomic::{AtomicBool, Ordering};

/// Is the relay pipe currently believed reachable? Set TRUE by the pipe task on WebSocket connect, FALSE on connect failure or a liveness break. Feeds the STRANDED edge: Wi-Fi Direct discovery only starts when a friend has undelivered traffic AND no validated path AND this is false (truly off-grid) — while the relay works, the relay is the cheaper fallback. Starts TRUE (optimistic) so a fresh boot doesn't spin the radio before the pipe has even tried.
pub static RELAY_REACHABLE: AtomicBool = AtomicBool::new(true);

pub fn relay_reachable() -> bool {
    RELAY_REACHABLE.load(Ordering::Relaxed)
}

pub fn set_relay_reachable(up: bool) {
    RELAY_REACHABLE.store(up, Ordering::Relaxed);
}

/// The Android group-owner address inside every Wi-Fi Direct group.
pub const WFD_GO_IP: std::net::Ipv4Addr = std::net::Ipv4Addr::new(192, 168, 49, 1);

// ---------------------------------------------------------------------------
// Key derivation — everything hangs off the friendship's relationship seed.
// ---------------------------------------------------------------------------

/// AEAD key sealing the `wfd_cred` payload between the two friends. Domain-separated off the relationship seed so neither the pong-seal key nor any history key is reused.
pub fn wfd_seal_key(relationship_seed: &[u8; 32]) -> [u8; 32] {
    *blake3::keyed_hash(relationship_seed, b"photon wfd_cred seal v1").as_bytes()
}

/// Keyed token base for discovery: only the two friends can derive it, so a token proves "someone holding our pairwise seed" without naming anyone.
fn wfd_token_key(relationship_seed: &[u8; 32]) -> [u8; 32] {
    *blake3::keyed_hash(relationship_seed, b"photon wfd token v1").as_bytes()
}

/// The rotating friend-recognizable discovery token: `keyed_hash(pairwise, device_pubkey ‖ coarse_hour)` truncated to 16 bytes. Rotates hourly so a stranger logging DNS-SD frames can't track a device across sightings; a friend derives the same token for the current (and previous, for clock skew) hour and matches.
pub const WFD_TOKEN_LEN: usize = 16;

pub fn wfd_token(
    relationship_seed: &[u8; 32],
    device_pubkey: &[u8; 32],
    coarse_hour: u64,
) -> [u8; WFD_TOKEN_LEN] {
    let key = wfd_token_key(relationship_seed);
    let mut msg = [0u8; 40];
    msg[..32].copy_from_slice(device_pubkey);
    msg[32..].copy_from_slice(&coarse_hour.to_le_bytes());
    let h = blake3::keyed_hash(&key, &msg);
    let mut t = [0u8; WFD_TOKEN_LEN];
    t.copy_from_slice(&h.as_bytes()[..WFD_TOKEN_LEN]);
    t
}

/// The coarse hour for token rotation, from eagle-time oscillations.
pub fn coarse_hour_now() -> u64 {
    // Eagle oscillations are ~9.19GHz-scale ticks; the exact epoch doesn't matter — both ends use the same function, and matching tries hour and hour−1 to absorb skew.
    (vsf::eagle_time_oscillations() as u64) / (3600 * vsf::OSCILLATIONS_PER_SECOND as u64)
}

/// Build the TXT-record token blob we advertise: one current-hour token per provisioned friend, concatenated. ~20 friends fit a conservative 400B TXT budget; beyond that the newest-provisioned win (deterministic, logged by the caller).
pub fn build_txt_tokens(entries: &[([u8; 32], [u8; 32])]) -> Vec<u8> {
    // entries: (relationship_seed, OUR device_pubkey) per friend — we advertise OUR token toward each friend.
    let hour = coarse_hour_now();
    let mut out = Vec::with_capacity(entries.len() * WFD_TOKEN_LEN);
    for (seed, our_dev) in entries.iter().take(25) {
        out.extend_from_slice(&wfd_token(seed, our_dev, hour));
    }
    out
}

/// Match a heard TXT blob against one friend: does any 16-byte chunk equal the token THAT friend's known device would advertise this hour (or last hour, for skew)? Returns the matching device pubkey.
pub fn match_txt_tokens(
    txt: &[u8],
    relationship_seed: &[u8; 32],
    friend_devices: &[[u8; 32]],
) -> Option<[u8; 32]> {
    let hour = coarse_hour_now();
    for dev in friend_devices {
        for h in [hour, hour.wrapping_sub(1)] {
            let want = wfd_token(relationship_seed, dev, h);
            if txt.chunks_exact(WFD_TOKEN_LEN).any(|c| c == want) {
                return Some(*dev);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The pre-provisioned per-pair credential.
// ---------------------------------------------------------------------------

/// The Wi-Fi Direct group credential a friend pair shares, minted by the lexicographically-LOWER device pubkey and sent sealed over the normal channel while online — so an offline meetup needs zero bootstrap radio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WfdCred {
    /// The designated group owner's device pubkey — deterministic tie-break, both sides agree with zero negotiation.
    pub go: [u8; 32],
    /// Group SSID; Wi-Fi Direct requires the `DIRECT-` prefix.
    pub ssid: String,
    /// Group passphrase (random printable).
    pub psk: String,
    /// Monotonic; a newer record replaces the older on both sides (rotate-on-use).
    pub epoch: u64,
}

/// Deterministic GO election for a pair: the lexicographically lower device pubkey owns the group. Lower wins so both ends compute it locally from keys they already hold.
pub fn elect_go(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    if a <= b {
        *a
    } else {
        *b
    }
}

/// Mint a fresh credential for a pair (called ONLY by the elected-GO side; the mint edge is friendship-established / post-teardown rotate / friendship-key epoch change).
pub fn mint_cred(our_device: &[u8; 32], their_device: &[u8; 32], epoch: u64) -> WfdCred {
    use rand::RngCore;
    let mut rng = rand::thread_rng();
    // SSID suffix: 8 chars of base32 (no padding chars, AP-name-safe).
    const B32: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut sfx = [0u8; 8];
    rng.fill_bytes(&mut sfx);
    let ssid: String = std::iter::once("DIRECT-ph-".to_string())
        .chain(sfx.iter().map(|b| (B32[(*b % 32) as usize] as char).to_string()))
        .collect();
    // PSK: 32 chars from a 62-symbol alphabet (~190 bits) — WPA2 passphrase-legal, far past brute-force relevance.
    const AL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut pb = [0u8; 32];
    rng.fill_bytes(&mut pb);
    let psk: String = pb.iter().map(|b| AL[(*b % 62) as usize] as char).collect();
    WfdCred {
        go: elect_go(our_device, their_device),
        ssid,
        psk,
        epoch,
    }
}

/// Encode the credential as a sealed blob under the pair's wfd seal key: inner `wfdcred` VSF section (headerless encrypted form, same shape as the pong tail), kete AEAD outside.
pub fn seal_cred(cred: &WfdCred, relationship_seed: &[u8; 32]) -> Result<Vec<u8>, String> {
    use vsf::VsfType;
    let mut inner = vsf::VsfSection::new("wfdcred");
    inner.add_field("go", VsfType::ke(cred.go.to_vec()));
    inner.add_field("ssid", VsfType::x(cred.ssid.clone()));
    inner.add_field("psk", VsfType::x(cred.psk.clone()));
    inner.add_field("epoch", VsfType::u(cred.epoch as usize, false));
    kete::encrypt_bytes(&inner.encode_encrypted(), &wfd_seal_key(relationship_seed))
}

/// Open + parse a sealed credential. AEAD failure (wrong pair, tamper) → Err; the caller drops it un-adopted.
pub fn open_cred(sealed: &[u8], relationship_seed: &[u8; 32]) -> Result<WfdCred, String> {
    use vsf::VsfType;
    let plain = kete::decrypt_bytes(sealed, &wfd_seal_key(relationship_seed))?;
    let mut ptr = 0;
    let section =
        vsf::VsfSection::parse(&plain, &mut ptr).map_err(|e| format!("wfdcred parse: {}", e))?;
    if section.name != "wfdcred" {
        return Err(format!("wfdcred name mismatch: {}", section.name));
    }
    let go = section
        .get_field("go")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::ke(b) if b.len() == 32 => {
                let mut k = [0u8; 32];
                k.copy_from_slice(b);
                Some(k)
            }
            _ => None,
        })
        .ok_or("wfdcred missing go")?;
    let text = |name: &str| -> Option<String> {
        section
            .get_field(name)
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::x(s) => Some(s.clone()),
                _ => None,
            })
    };
    let ssid = text("ssid").ok_or("wfdcred missing ssid")?;
    if !ssid.starts_with("DIRECT-") {
        return Err("wfdcred ssid lacks the DIRECT- prefix".into());
    }
    let psk = text("psk").ok_or("wfdcred missing psk")?;
    let epoch = section
        .get_field("epoch")
        .and_then(|f| f.values.first())
        .and_then(|v| v.as_u64())
        .ok_or("wfdcred missing epoch")?;
    Ok(WfdCred {
        go,
        ssid,
        psk,
        epoch,
    })
}

// ---------------------------------------------------------------------------
// Bearer state machine — edge-driven, no timers (house rule).
// ---------------------------------------------------------------------------

/// The bearer's lifecycle. Transitions fire on EDGES evaluated by the app drain, never on timers:
/// Idle → Stranded on STRANDED-ENTER (a friend has undelivered rows ∧ no validated path ∧ relay unreachable) — start advertise + discovery.
/// Stranded → Forming on FRIEND-HEARD (DNS-SD token matched a provisioned friend with no path) — createGroup if we're the cred's GO, else connect toward the GO.
/// Forming → Up on GROUP-UP (platform up-call with the interface addresses) — fire the unicast pt_disc exchange; normal punch validation adopts the path.
/// Up → Idle on DRAINED+SILENT or IFACE-LOST — remove the group / clear p2p addresses so sends fall back instead of black-holing.
/// Any → Idle on STRANDED-EXIT (every triggering friend gained a path or drained).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WfdState {
    #[default]
    Idle,
    Stranded,
    Forming,
    Up,
}

/// What the platform radio bridge must provide. Android implements this over WifiP2pManager via the PhotonWifiDirect Kotlin courier; Linux over wpa_supplicant's P2PDevice D-Bus interface (feature `wfd-linux`); everywhere else the no-op default applies and the tier simply never engages.
pub trait WfdPlatform: Send {
    /// Register our DNS-SD local service with this TXT token blob and start service discovery. Idempotent.
    fn start(&mut self, txt_tokens: &[u8]);
    /// Stop advertise + discovery (leaves an established group alone).
    fn stop(&mut self);
    /// We are the elected GO: create the group with pre-shared credentials (dialog-free on API 29+).
    fn create_group(&mut self, ssid: &str, psk: &str);
    /// We are the joiner: connect to the friend's group with the pre-shared credentials.
    fn connect_group(&mut self, ssid: &str, psk: &str);
    /// Tear the group down (DRAINED+SILENT edge).
    fn remove_group(&mut self);
}

/// The everywhere-else default: the tier never engages.
pub struct NullWfd;

impl WfdPlatform for NullWfd {
    fn start(&mut self, _txt_tokens: &[u8]) {}
    fn stop(&mut self) {}
    fn create_group(&mut self, _ssid: &str, _psk: &str) {}
    fn connect_group(&mut self, _ssid: &str, _psk: &str) {}
    fn remove_group(&mut self) {}
}

/// The bearer: current state + the platform bridge. Owned by the app (UI thread), driven by its drain on the documented edges.
pub struct WfdBearer {
    pub state: WfdState,
    platform: Box<dyn WfdPlatform>,
}

impl WfdBearer {
    pub fn new(platform: Box<dyn WfdPlatform>) -> Self {
        Self {
            state: WfdState::Idle,
            platform,
        }
    }

    /// STRANDED-ENTER / STRANDED-EXIT evaluation: `any_stranded` is the app's answer to "does any provisioned friend have undelivered rows, no validated path, and no relay?". Called on the edges that can change that answer (row queued, path validated/lost, relay state flip), not on a timer.
    pub fn eval_stranded(&mut self, any_stranded: bool, txt_tokens: &[u8]) {
        match (self.state, any_stranded) {
            (WfdState::Idle, true) => {
                crate::log("WFD: stranded — starting service advertise + discovery");
                self.platform.start(txt_tokens);
                self.state = WfdState::Stranded;
            }
            (WfdState::Stranded, false) => {
                crate::log("WFD: no longer stranded — stopping advertise + discovery");
                self.platform.stop();
                self.state = WfdState::Idle;
            }
            _ => {}
        }
    }

    /// FRIEND-HEARD: a token matched `their_device` and we hold `cred` for that pair. `our_device` decides the role.
    pub fn friend_heard(&mut self, our_device: &[u8; 32], cred: &WfdCred) {
        if self.state != WfdState::Stranded {
            return;
        }
        if &cred.go == our_device {
            crate::logf!("WFD: friend nearby — we are GO, creating group {}", cred.ssid);
            self.platform.create_group(&cred.ssid, &cred.psk);
        } else {
            crate::logf!("WFD: friend nearby — joining group {}", cred.ssid);
            self.platform.connect_group(&cred.ssid, &cred.psk);
        }
        self.state = WfdState::Forming;
    }

    /// GROUP-UP: the platform reported a formed group. The caller (app drain) fires the pt_disc unicast exchange with the addresses it got from the StatusUpdate.
    pub fn group_up(&mut self) {
        self.state = WfdState::Up;
    }

    /// DRAINED+SILENT: traffic drained and the p2p path went invalid — tear down.
    pub fn drained(&mut self) {
        if self.state == WfdState::Up {
            crate::log("WFD: drained + silent — removing group");
            self.platform.remove_group();
        }
        self.platform.stop();
        self.state = WfdState::Idle;
    }

    /// IFACE-LOST: the platform reported the group gone (out of range / OS removal). The caller clears p2p addresses; we just reset.
    pub fn iface_lost(&mut self) {
        self.state = WfdState::Idle;
    }
}

// ---------------------------------------------------------------------------
// Process-wide bearer + platform event queue. The Kotlin/D-Bus radio bridges up-call from their own threads; events queue here and the UI drain converts them to StatusUpdates. The bearer itself is a singleton because the radio is (one P2P device per phone).
// ---------------------------------------------------------------------------

/// A platform radio event, queued by the JNI up-calls and drained by the app's status pass.
pub enum WfdEvent {
    /// A DNS-SD response's TXT token blob — matched against provisioned friends on the UI thread (which holds the contacts).
    ServiceFound(Vec<u8>),
    /// Group formation state changed.
    GroupChanged {
        formed: bool,
        is_go: bool,
        our_ip: std::net::Ipv4Addr,
        go_ip: std::net::Ipv4Addr,
    },
}

static EVENTS: std::sync::Mutex<Vec<WfdEvent>> = std::sync::Mutex::new(Vec::new());
static BEARER: std::sync::Mutex<Option<WfdBearer>> = std::sync::Mutex::new(None);

pub fn push_event(e: WfdEvent) {
    EVENTS.lock().unwrap().push(e);
}

pub fn drain_events() -> Vec<WfdEvent> {
    std::mem::take(&mut EVENTS.lock().unwrap())
}

fn platform_default() -> Box<dyn WfdPlatform> {
    #[cfg(target_os = "android")]
    {
        return crate::platform::jni_android::android_wfd_platform();
    }
    #[allow(unreachable_code)]
    Box::new(NullWfd)
}

fn with_bearer<R>(f: impl FnOnce(&mut WfdBearer) -> R) -> R {
    let mut g = BEARER.lock().unwrap();
    f(g.get_or_insert_with(|| WfdBearer::new(platform_default())))
}

pub fn bearer_state() -> WfdState {
    with_bearer(|b| b.state)
}

/// STRANDED evaluation entry point (called from the app's sync driver): `entries` = (relationship_seed, OUR device pubkey) per provisioned friend; the TXT blob is built here.
pub fn eval_stranded(any_stranded: bool, entries: &[([u8; 32], [u8; 32])]) {
    let txt = if any_stranded {
        build_txt_tokens(entries)
    } else {
        Vec::new()
    };
    with_bearer(|b| b.eval_stranded(any_stranded, &txt));
}

pub fn friend_heard(our_device: &[u8; 32], cred: &WfdCred) {
    with_bearer(|b| b.friend_heard(our_device, cred));
}

pub fn group_up() {
    with_bearer(|b| b.group_up());
}

pub fn drained() {
    with_bearer(|b| b.drained());
}

pub fn iface_lost() {
    with_bearer(|b| b.iface_lost());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_election_is_deterministic_and_symmetric() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        assert_eq!(elect_go(&a, &b), elect_go(&b, &a));
        assert_eq!(elect_go(&a, &b), a);
        assert_eq!(elect_go(&a, &a), a);
    }

    #[test]
    fn cred_seal_roundtrip() {
        let seed = [7u8; 32];
        let cred = mint_cred(&[1u8; 32], &[2u8; 32], 3);
        assert!(cred.ssid.starts_with("DIRECT-ph-"));
        assert_eq!(cred.psk.len(), 32);
        let sealed = seal_cred(&cred, &seed).unwrap();
        let opened = open_cred(&sealed, &seed).unwrap();
        assert_eq!(opened, cred);
    }

    #[test]
    fn cred_wrong_seed_fails_closed() {
        let cred = mint_cred(&[1u8; 32], &[2u8; 32], 1);
        let sealed = seal_cred(&cred, &[7u8; 32]).unwrap();
        assert!(open_cred(&sealed, &[8u8; 32]).is_err());
    }

    #[test]
    fn tokens_match_only_with_the_pair_seed() {
        let seed = [9u8; 32];
        let dev = [4u8; 32];
        let txt = {
            let hour = coarse_hour_now();
            let mut t = wfd_token(&seed, &dev, hour).to_vec();
            t.extend_from_slice(&wfd_token(&[1u8; 32], &[5u8; 32], hour)); // someone else's token
            t
        };
        assert_eq!(match_txt_tokens(&txt, &seed, &[dev]), Some(dev));
        assert_eq!(match_txt_tokens(&txt, &[6u8; 32], &[dev]), None);
    }

    #[test]
    fn state_machine_edges() {
        let mut b = WfdBearer::new(Box::new(NullWfd));
        assert_eq!(b.state, WfdState::Idle);
        b.eval_stranded(true, &[]);
        assert_eq!(b.state, WfdState::Stranded);
        let cred = mint_cred(&[1u8; 32], &[2u8; 32], 1);
        b.friend_heard(&[1u8; 32], &cred);
        assert_eq!(b.state, WfdState::Forming);
        b.group_up();
        assert_eq!(b.state, WfdState::Up);
        b.drained();
        assert_eq!(b.state, WfdState::Idle);
        // Stranded-exit from Stranded:
        b.eval_stranded(true, &[]);
        b.eval_stranded(false, &[]);
        assert_eq!(b.state, WfdState::Idle);
    }
}
