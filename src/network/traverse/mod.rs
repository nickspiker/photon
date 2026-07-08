//! NAT traversal — turning a peer identity into a working socket address.
//!
//! Photon's transport (PT) assumes it already has a reachable `SocketAddr`; this module is what produces
//! one. It assembles the pieces photon already had (a dual-stack socket, signed contact-gated ping/pong,
//! PT's address racing) into a real connection-establishment handshake: gather candidates → learn our
//! reflexive address → exchange candidates → coordinated simultaneous hole-punch → validate a working
//! path → hand it to PT. See the traversal plan for the milestone breakdown.
//!
//! Two trust tiers run through here:
//! - **Friend tier** (data plane): punch-for-delivery, contact/fleet-gated exactly like ping.
//! - **Directory tier** (open substrate): address reflection + phonebook serving, open to any node under
//!   the "serve directory" setting, safe because trustless (self-signed records, reflection reveals only
//!   the requester's own address).
//!
//! Built in phases; modules appear as each phase lands.

pub mod candidate;
pub mod gather;
pub mod punch;
pub mod reflexive;
pub mod session;
