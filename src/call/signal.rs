//! Call signaling — encrypted control rows ON THE LANES, never a bare wire frame.
//!
//! An offer/answer/hangup is ordinary lane-sealed message content carrying the [`crate::types::CALL_PREFIX`] marker: to every relay, queue, and wire observer a call is indistinguishable from a text message. What that buys, for free: fold trust, receive-anywhere (every callee device decrypts the offer → every device can ring and any can answer), dedup, retransmit, and the offer's lane key falling out of the decrypt as the basket's doomed egg.
//!
//! Record grammar (STX-separated fields after the prefix, the attachment-row convention):
//! `PREFIX kind \u{2} call_id_hex32 [\u{2} nonce_hex64]` — nonce present on offer/answer only.
//!
//! These rows are CONTROL content: hidden from every surface, never dinged by the normal path (the RING is its own edge at offer receipt, bypassing claim/attention suppression — a call is the one always-ring event). The visible record of a call (missed/completed/duration) is a separate summary row minted at the end.

use crate::types::CALL_PREFIX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSignal {
    Offer { call_id: [u8; 16], nonce: [u8; 32] },
    Answer { call_id: [u8; 16], nonce: [u8; 32] },
    /// Callee refused. Ring stops fleet-wide (the decline fans to the callee's siblings as a row like any other).
    Decline { call_id: [u8; 16] },
    /// Callee is already in a call — automatic, not a human edge.
    Busy { call_id: [u8; 16] },
    /// Either side ended it (also the caller's give-up on an unanswered ring — the human IS the timeout).
    Hangup { call_id: [u8; 16] },
    /// Caller → a losing answerer: another device won the race.
    Taken { call_id: [u8; 16] },
}

impl CallSignal {
    pub fn call_id(&self) -> &[u8; 16] {
        match self {
            CallSignal::Offer { call_id, .. }
            | CallSignal::Answer { call_id, .. }
            | CallSignal::Decline { call_id }
            | CallSignal::Busy { call_id }
            | CallSignal::Hangup { call_id }
            | CallSignal::Taken { call_id } => call_id,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            CallSignal::Offer { .. } => "offer",
            CallSignal::Answer { .. } => "answer",
            CallSignal::Decline { .. } => "decline",
            CallSignal::Busy { .. } => "busy",
            CallSignal::Hangup { .. } => "hangup",
            CallSignal::Taken { .. } => "taken",
        }
    }

    /// The row content string this signal rides as.
    pub fn to_content(&self) -> String {
        let base = format!("{}{}\u{2}{}", CALL_PREFIX, self.kind(), hex::encode(self.call_id()));
        match self {
            CallSignal::Offer { nonce, .. } | CallSignal::Answer { nonce, .. } => {
                format!("{}\u{2}{}", base, hex::encode(nonce))
            }
            _ => base,
        }
    }

    /// Parse a row's content. None for non-call content or a malformed record (malformed = dropped, never guessed).
    pub fn parse(content: &str) -> Option<CallSignal> {
        let rest = content.strip_prefix(CALL_PREFIX)?;
        let mut parts = rest.split('\u{2}');
        let kind = parts.next()?;
        let call_id: [u8; 16] = hex::decode(parts.next()?).ok()?.try_into().ok()?;
        let nonce: Option<[u8; 32]> = parts
            .next()
            .and_then(|h| hex::decode(h).ok())
            .and_then(|b| b.try_into().ok());
        match (kind, nonce) {
            ("offer", Some(nonce)) => Some(CallSignal::Offer { call_id, nonce }),
            ("answer", Some(nonce)) => Some(CallSignal::Answer { call_id, nonce }),
            ("decline", None) => Some(CallSignal::Decline { call_id }),
            ("busy", None) => Some(CallSignal::Busy { call_id }),
            ("hangup", None) => Some(CallSignal::Hangup { call_id }),
            ("taken", None) => Some(CallSignal::Taken { call_id }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_round_trip_and_are_control() {
        let id = [0xAB; 16];
        let n = [0xCD; 32];
        for sig in [
            CallSignal::Offer { call_id: id, nonce: n },
            CallSignal::Answer { call_id: id, nonce: n },
            CallSignal::Decline { call_id: id },
            CallSignal::Busy { call_id: id },
            CallSignal::Hangup { call_id: id },
            CallSignal::Taken { call_id: id },
        ] {
            let content = sig.to_content();
            assert_eq!(CallSignal::parse(&content), Some(sig));
            assert!(
                crate::types::is_control_content(&content),
                "call signals must be hidden machinery rows"
            );
        }
        assert_eq!(CallSignal::parse("hello"), None);
        // A truncated offer (missing nonce) is malformed, not a lesser signal.
        let bad = format!("{}offer\u{2}{}", CALL_PREFIX, hex::encode([1u8; 16]));
        assert_eq!(CallSignal::parse(&bad), None);
    }
}
