//! Typed event bus with lossy and reliable delivery.
//!
//! Two delivery policies, because they answer different questions:
//!
//! - [`EventBus::publish`] is **lossy**: a slow subscriber loses its oldest
//!   event (telemetry — a stale reading has no value, and blocking the producer
//!   on a slow consumer is worse);
//! - [`EventBus::try_publish`] is **reliable**: it either delivers to every
//!   subscriber or to none, failing when any queue is full (faults, OTA
//!   progress — losing one is not acceptable, and the producer should know).
//!
//! Subscribers are fixed at boot, no allocation, and each has an independent
//! cursor, so a slow consumer cannot slow another.

use crate::ipc::MessageQueue;

/// A fixed set of subscriber queues.
pub struct EventBus<T: Copy + Default, const N: usize, const S: usize> {
    queues: [MessageQueue<T, N>; S],
    subscribers: usize,
    published: u64,
    dropped: u64,
}

impl<T: Copy + Default, const N: usize, const S: usize> Default for EventBus<T, N, S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default, const N: usize, const S: usize> EventBus<T, N, S> {
    /// Create a bus with no subscribers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            queues: core::array::from_fn(|_| MessageQueue::new()),
            subscribers: 0,
            published: 0,
            dropped: 0,
        }
    }

    /// Subscribe, returning the subscriber id. `None` when all slots are used.
    pub fn subscribe(&mut self) -> Option<usize> {
        if self.subscribers >= S {
            return None;
        }
        let id = self.subscribers;
        self.subscribers += 1;
        Some(id)
    }

    /// Number of subscribers.
    #[must_use]
    pub const fn subscribers(&self) -> usize {
        self.subscribers
    }

    /// Publish lossy: every subscriber receives the event; a full queue drops
    /// its oldest event. Returns how many events were dropped.
    pub fn publish(&mut self, event: T) -> u64 {
        self.published = self.published.saturating_add(1);
        let mut dropped = 0;
        for queue in self.queues[..self.subscribers].iter_mut() {
            if queue.push_overwrite(event).is_some() {
                dropped += 1;
            }
        }
        self.dropped = self.dropped.saturating_add(dropped);
        dropped
    }

    /// Publish reliably: deliver to every subscriber, or to none.
    ///
    /// Returns `false` when at least one subscriber queue is full; nothing is
    /// enqueued in that case.
    pub fn try_publish(&mut self, event: T) -> bool {
        if self.queues[..self.subscribers]
            .iter()
            .any(MessageQueue::is_full)
        {
            return false;
        }
        for queue in self.queues[..self.subscribers].iter_mut() {
            let _ = queue.push(event);
        }
        self.published = self.published.saturating_add(1);
        true
    }

    /// Pop the next event for a subscriber.
    pub fn pop(&mut self, subscriber: usize) -> Option<T> {
        self.queues.get_mut(subscriber).and_then(MessageQueue::pop)
    }

    /// Queue depth for a subscriber.
    #[must_use]
    pub fn len(&self, subscriber: usize) -> usize {
        self.queues.get(subscriber).map_or(0, MessageQueue::len)
    }

    /// Whether a subscriber has no pending events.
    #[must_use]
    pub fn is_empty(&self, subscriber: usize) -> bool {
        self.len(subscriber) == 0
    }

    /// `(published, dropped)` counters from boot.
    #[must_use]
    pub const fn stats(&self) -> (u64, u64) {
        (self.published, self.dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribers_are_independent() {
        let mut bus = EventBus::<u32, 4, 2>::new();
        let a = bus.subscribe().unwrap();
        let b = bus.subscribe().unwrap();
        assert_eq!(bus.subscribe(), None);

        assert_eq!(bus.publish(7), 0);
        assert_eq!(bus.pop(a), Some(7));
        assert_eq!(bus.pop(b), Some(7));
        assert!(bus.is_empty(a));
    }

    #[test]
    fn lossy_publish_drops_the_oldest() {
        let mut bus = EventBus::<u32, 2, 1>::new();
        let sub = bus.subscribe().unwrap();
        assert_eq!(bus.publish(1), 0);
        assert_eq!(bus.publish(2), 0);
        assert_eq!(bus.publish(3), 1); // 1 is dropped
        assert_eq!(bus.pop(sub), Some(2));
        assert_eq!(bus.pop(sub), Some(3));
        assert_eq!(bus.stats(), (3, 1));
    }

    #[test]
    fn reliable_publish_is_all_or_nothing() {
        let mut bus = EventBus::<u32, 1, 2>::new();
        let a = bus.subscribe().unwrap();
        let _b = bus.subscribe().unwrap();
        assert!(bus.try_publish(10));
        assert!(!bus.try_publish(11)); // no room for either subscriber
        assert_eq!(bus.pop(a), Some(10));
    }

    #[test]
    fn publishing_without_subscribers_is_harmless() {
        let mut bus = EventBus::<u32, 2, 1>::new();
        assert_eq!(bus.publish(1), 0);
        assert_eq!(bus.stats(), (1, 0));
    }
}
