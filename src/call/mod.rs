//! Voice calls (docs/calls.md) — 1:1, fleet-native, wire-invisible.
//!
//! The three planes: SIGNALING rides the friendship lanes as encrypted control rows (a call is indistinguishable from a message on the wire — no relay ever learns a call happened); MEDIA is an ephemeral UDP plane under a basket-derived key ([`keys`]); HISTORY is ordinary rows (missed/completed/duration) plus the optional kept recording on the attachment plane.
//!
//! No timers anywhere: ringing stops on answer/decline/hangup edges, the caller's patience is the timeout, and the intra-call key ratchet steps on packet COUNT, not clocks.

pub mod keys;
