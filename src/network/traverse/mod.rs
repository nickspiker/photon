//! NAT traversal — turning a peer identity into a working socket address.
//!
//! Photon's transport (PT) assumes it already has a reachable `SocketAddr`; this module is what produces one. It assembles the pieces photon already had (a dual-stack socket, signed contact-gated ping/pong, PT's address racing) into a real connection-establishment handshake: gather candidates → learn our reflexive address → exchange candidates → coordinated simultaneous hole-punch → validate a working path → hand it to PT. See the traversal plan for the milestone breakdown.
//!
//! Two trust tiers run thru here:
//! - **Friend tier** (data plane): punch-for-delivery, contact/fleet-gated exactly like ping.
//! - **Directory tier** (open substrate): address reflection + phonebook serving, open to any node under the "serve directory" setting, safe because trustless (self-signed records, reflection reveals only the requester's own address).
//!
//! # Where this code lives now
//!
//! The transport-agnostic half lives in the `fgtw` crate's `traverse` module so that photon and
//! rustdesk share one implementation instead of two that drift. `candidate`, `reflexive` and
//! `session` are straight re-exports; [`gather`] stays local because it adapts photon's
//! `Contact` onto the crate's endpoint shape; [`punch`] stays local for now because it encodes
//! into `FgtwMessage`.
//!
//! Photon drives the crate's state machines from its own receive loop in `network::status`,
//! which multiplexes one socket across the whole data plane. It therefore uses the state
//! machines but NOT the crate's `driver` module — rustdesk, which has no such loop, uses the
//! driver instead. That asymmetry is deliberate; see the crate docs before "fixing" it.

pub mod gather;
pub mod punch;

pub use fgtw::traverse::{candidate, reflexive, session};
