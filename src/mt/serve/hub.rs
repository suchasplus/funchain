//! Tiny pub-sub broadcaster for live-reload events.

use crossbeam_channel::{Receiver, Sender, unbounded};
use std::sync::Mutex;

/// Broadcasts a `()` "reload now" pulse to every subscriber. Dead receivers
/// are pruned lazily on the next broadcast.
#[derive(Default)]
pub struct Hub {
    subs: Mutex<Vec<Sender<()>>>,
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new subscriber. The returned `Receiver` drains `()` pulses.
    pub fn subscribe(&self) -> Receiver<()> {
        let (tx, rx) = unbounded();
        self.subs.lock().expect("hub poisoned").push(tx);
        rx
    }

    /// Fan a single pulse to every live subscriber.
    pub fn broadcast(&self) {
        let mut subs = self.subs.lock().expect("hub poisoned");
        // try_send fails when receiver is dropped or unbounded channel is shut down.
        subs.retain(|s| s.try_send(()).is_ok());
    }

    /// Current number of live subscribers (for tests/diagnostics).
    pub fn subscriber_count(&self) -> usize {
        self.subs.lock().expect("hub poisoned").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn broadcast_reaches_all_subscribers() {
        let hub = Hub::new();
        let a = hub.subscribe();
        let b = hub.subscribe();
        hub.broadcast();
        assert!(a.recv_timeout(Duration::from_millis(50)).is_ok());
        assert!(b.recv_timeout(Duration::from_millis(50)).is_ok());
    }

    #[test]
    fn dropped_subscribers_get_pruned_on_next_broadcast() {
        let hub = Hub::new();
        {
            let _rx = hub.subscribe();
        } // dropped here
        assert_eq!(hub.subscriber_count(), 1);
        hub.broadcast();
        assert_eq!(hub.subscriber_count(), 0);
    }

    #[test]
    fn broadcast_is_lossless_when_subscribers_drain() {
        let hub = Hub::new();
        let rx = hub.subscribe();
        for _ in 0..5 {
            hub.broadcast();
        }
        for _ in 0..5 {
            rx.recv_timeout(Duration::from_millis(50)).unwrap();
        }
    }
}
