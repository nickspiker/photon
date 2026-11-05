//! THE RANK (Nick 2026-11-05, after Theresa's wave fell to 16 kbps while her phone served a backlog of history and attachments up the same thin uplink): every byte this device sends has a class, and a lower class gives way to a higher one.
//!
//! The classes, highest first:
//! - realtime — a wave's voice, then a beam's video: sent the moment it exists, never queued behind anything (wave::send_media goes straight to the socket);
//! - control — pings, acks, ceremony frames: single packets outside the transfer pacer;
//! - interactive — chat rows: small transfers, at most [`INTERACTIVE_MAX`] bytes, paced by the path but never held for a call;
//! - bulk — history pages, attachment chunks, backfill: everything larger, and the only class a call squeezes.
//!
//! Strict order with a floor, never fixed shares: a voice wants 16 to 128 kbps whatever the link, so a share of a fat link would waste it and a share of a thin one might starve it.
//! While any realtime stream is live on this device, bulk on every path may hold at most 1/2^[`BULK_FLOOR_LOG2`] of that path's delay-based window in flight, so history and attachments still crawl forward and the call keeps the rest.
//! Device-wide, not per path: the device's uplink is the bottleneck every path shares, and Theresa's backlog went to Nick's Pixel while the wave it starved went to his desktop.
//! The edge is the stream's own thread: a wave engine or a beam sender takes a [`RealtimeGuard`] for its lifetime, so the squeeze starts with the first frame and ends with the last, with no clock.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Realtime streams live on this device: wave engines (one per link of a group wave) and beam senders.
static REALTIME_LIVE: AtomicUsize = AtomicUsize::new(0);

/// A transfer of at most this many bytes is interactive (a chat row with its envelope is a few KB); a larger one is bulk.
pub const INTERACTIVE_MAX: u32 = 1 << 14;

/// Bulk's floor while a call is live, as log2 of the share of a path's window: 4 = one sixteenth.
pub const BULK_FLOOR_LOG2: f32 = 4.;

/// Held by a realtime stream's thread for as long as it sends.
pub struct RealtimeGuard(());

impl Drop for RealtimeGuard {
    fn drop(&mut self) {
        if REALTIME_LIVE.fetch_sub(1, Ordering::AcqRel) == 1 {
            crate::log("RANK: the last realtime stream ended — bulk transfers take the whole window again");
        }
    }
}

/// A realtime stream starts on this thread; the returned guard ends it.
pub fn realtime_begin() -> RealtimeGuard {
    if REALTIME_LIVE.fetch_add(1, Ordering::AcqRel) == 0 {
        crate::logf!("RANK: a realtime stream is live — bulk transfers drop to 1/{} of each path's window until it ends", 1u32 << BULK_FLOOR_LOG2 as u32);
    }
    RealtimeGuard(())
}

/// Is any wave or beam sending from this device right now?
pub fn realtime_live() -> bool {
    REALTIME_LIVE.load(Ordering::Acquire) > 0
}

/// A transfer of `bytes` is bulk (squeezed during a call) rather than interactive.
pub fn is_bulk(bytes: u32) -> bool {
    bytes > INTERACTIVE_MAX
}

/// How many of a path's in-flight packets bulk may hold now, given the path's window in packets: the whole window, or its floor share while a call is live (at least one, so bulk never stalls outright).
pub fn bulk_allowance(window: f32) -> f32 {
    if realtime_live() {
        // WHY/PROOF: a window under 16 packets would leave bulk an allowance under one, and a transfer that may never send a packet never completes, so history and attachments would stall for the whole call instead of crawling.
        (window / BULK_FLOOR_LOG2.exp2()).max(1.)
    } else {
        window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_stream_squeezes_bulk_to_its_floor_and_releases_it() {
        assert!(!realtime_live());
        assert_eq!(bulk_allowance(64.), 64.);
        let g = realtime_begin();
        assert!(realtime_live());
        assert_eq!(bulk_allowance(64.), 4.);
        assert_eq!(bulk_allowance(3.), 1., "the floor never falls below one packet");
        let g2 = realtime_begin();
        drop(g);
        assert!(realtime_live(), "a second stream keeps the squeeze");
        drop(g2);
        assert!(!realtime_live());
        assert_eq!(bulk_allowance(64.), 64.);
    }

    #[test]
    fn small_transfers_are_interactive() {
        assert!(!is_bulk(1 << 12));
        assert!(!is_bulk(INTERACTIVE_MAX));
        assert!(is_bulk(INTERACTIVE_MAX + 1));
    }
}
