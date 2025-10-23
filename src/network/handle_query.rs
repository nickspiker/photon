// Handle query protocol for checking handle attestation status
//
// Network layer for querying the distributed hash table (DHT) to check if a handle
// has been attested (claimed) or is available.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::Duration;

/// Result of a handle query
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryResult {
    Unattested,      // Handle is available
    AlreadyAttested, // Handle is already claimed
}

/// Handle query request/response channel
pub struct HandleQuery {
    sender: Sender<String>,
    receiver: Receiver<QueryResult>,
}

impl HandleQuery {
    /// Create a new handle query system
    pub fn new() -> Self {
        let (tx_request, rx_request) = channel::<String>();
        let (tx_response, rx_response) = channel::<QueryResult>();

        // Spawn worker thread to handle queries
        thread::spawn(move || {
            while let Ok(handle) = rx_request.recv() {
                // TODO: Implement actual DHT query
                // For now, simulate network delay and mock response
                thread::sleep(Duration::from_millis(500));

                // Mock logic: handles starting with vowels are "attested"
                let first_char = handle.chars().next().unwrap_or('a').to_ascii_lowercase();
                let result = if "aeiou".contains(first_char) {
                    QueryResult::AlreadyAttested
                } else {
                    QueryResult::Unattested
                };

                // Send response back
                let _ = tx_response.send(result);
            }
        });

        Self {
            sender: tx_request,
            receiver: rx_response,
        }
    }

    /// Query a handle (non-blocking)
    pub fn query(&self, handle: String) {
        let _ = self.sender.send(handle);
    }

    /// Check if a response is ready (non-blocking)
    pub fn try_recv(&self) -> Option<QueryResult> {
        self.receiver.try_recv().ok()
    }
}
