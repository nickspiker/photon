//! One process-wide HTTP stack for all FGTW traffic.
//!
//! reqwest's contract is to build a `Client` ONCE and reuse it: the client owns the connection pool, so reusing it keeps TLS sessions warm — handshake once per host, then HTTP/2-multiplex — instead of re-handshaking on every `Client::new()`. Tokio is the same story: a connection pool only stays warm as long as the reactor that owns it lives, so one persistent runtime beats a throwaway `block_on` runtime per call.
//!
//! - Async network code runs on [`runtime`] (one persistent multi-thread runtime) and uses [`async_client`]; their pool survives across calls.
//! - Genuinely blocking call sites — each on its own OS thread, never inside [`runtime`] — use [`blocking`], whose own internal runtime + pool persist for the process.
//!
//! Never call [`blocking`] from a task running on [`runtime`]: `reqwest::blocking` panics if it detects an active runtime. Keep the two halves on separate threads.
//!
//! Per-request timeouts are set at the call site with `.timeout(…)`, since they vary by operation; the shared clients carry no client-level timeout.

use std::sync::OnceLock;

// ════════════════════════════════════════════════════════════════════════════════════════════ THE SEED. One place that knows where fgtw.org is.
//
// fgtw.org is BOOTSTRAP SCAFFOLDING, not infrastructure: the end state is peers first, a random sample to confirm the list is current, and the seed consulted LAST — then retired entirely (docs/peers-are-fgtw.md). That ordering is impossible to implement while the host is spelled out at thirteen call sites, because "try the seed last" needs somewhere that can decide to try it at all. This is that somewhere.
//
// Keep it a single definition even though the constants look trivially duplicable — the whole point is that switching to a seed LIST, a user-configured seed, or no seed at all becomes one edit here rather than a hunt through six modules and seven inline string literals.
// ════════════════════════════════════════════════════════════════════════════════════════════

/// The bootstrap seed's bare host.
pub const SEED_HOST: &str = "fgtw.org";
/// The seed's HTTPS origin — every VSF POST target.
pub const SEED_HTTPS: &str = "https://fgtw.org";
/// The seed's broadcast hub (peer-IP push, pair events).
pub const SEED_WS: &str = "wss://fgtw.org/ws";
/// The seed's liveness probe — the one endpoint that says "is the seed reachable" without a payload.
pub const SEED_STATUS: &str = "https://fgtw.org/status";

/// This device's relay-pipe socket on the seed, keyed by its own device pubkey.
pub fn seed_pipe_url(device_pubkey_hex: &str) -> String {
    format!("wss://{SEED_HOST}/pipe?dev={device_pubkey_hex}")
}

/// The process-wide async runtime. Every FGTW `block_on` / spawn uses this one, so reqwest's connection pool stays warm across calls. Multi-thread so the worker threads (query, status, …) can `block_on` it concurrently.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("build process-wide tokio runtime")
    })
}

/// The shared async reqwest client — pools connections on [`runtime`]'s reactor.
pub fn async_client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .build()
            .expect("build shared async reqwest client")
    })
}

/// The shared blocking reqwest client — its own internal runtime + pool persist for the process. For call sites on dedicated OS threads only; never from within a [`runtime`] task.
pub fn blocking() -> &'static reqwest::blocking::Client {
    static C: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .build()
            .expect("build shared blocking reqwest client")
    })
}

/// The shared blocking client, for call sites that need their OWN per-request timeout.
/// Those sites used to each build a private `Client`, which throws away the connection pool and the warm TLS session — every call paid a fresh TCP + TLS handshake. On fibre that hides in the noise; on a 202 KB/s uplink it was most of a 7.3-second blob upload that blocked attestation (2026-07-29). reqwest hangs the timeout on the REQUEST BUILDER, so the pooled client and a per-call deadline are not mutually exclusive — `http::blocking_timeout(d).post(url)` keeps the pool AND the timeout. Prefer this over `Client::builder().timeout(d)`.
pub fn blocking_timeout(d: std::time::Duration) -> RequestTimeout {
    RequestTimeout(d)
}

/// Builder shim from [`blocking_timeout`] — mirrors the `Client` methods the FGTW call sites use, attaching the deadline to each request it hands back.
pub struct RequestTimeout(std::time::Duration);

impl RequestTimeout {
    pub fn post(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        blocking().post(url).timeout(self.0)
    }
    pub fn get(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        blocking().get(url).timeout(self.0)
    }
}

/// Short, plain message for a failed FGTW request — NO web-stack jargon (no "error sending request for url", no reqwest internals, no TCP/DNS strings the user can't act on). `action` is a short verb phrase like "reach FGTW" or "check the handle".
/// Connect failure and TIMEOUT are deliberately NOT the same message. A connect failure means we never reached the server. A timeout means we DID — the request went out and the reply didn't come back in time, which is the server being slow on that path, not the network being down. Collapsing both into "No connection to FGTW" printed that line while the connectivity probe held FGTW ONLINE and the UI showed a green circle (observed 2026-07-29 05:06:40: challenge answered in 1ms, the 398B announce then timed out at exactly 10.000s while GET /status kept succeeding) — which sends you debugging your network instead of the announce path.
pub fn short_send_error(action: &str, e: &reqwest::Error) -> String {
    if e.is_connect() {
        "No connection to FGTW".to_string()
    } else if e.is_timeout() {
        format!("FGTW didn't answer in time ({action})")
    } else {
        format!("Couldn't {action}")
    }
}
