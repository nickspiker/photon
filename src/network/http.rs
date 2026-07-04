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

/// Short, plain message for a failed FGTW request — NO web-stack jargon (no "error sending request for url", no reqwest internals, no TCP/DNS strings the user can't act on). A connect/timeout failure is the "server unreachable" case → "No connection to FGTW"; anything else is a plain per-action failure. `action` is a short verb phrase like "reach FGTW" or "check the handle".
pub fn short_send_error(action: &str, e: &reqwest::Error) -> String {
    if e.is_connect() || e.is_timeout() {
        "No connection to FGTW".to_string()
    } else {
        format!("Couldn't {action}")
    }
}
