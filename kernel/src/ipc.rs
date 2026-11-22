//! Inter-process communication primitives.
//!
//! The Linux kernel offers pipes, message queues, shared memory, semaphores and
//! futexes; a 512 KiB MCU cannot afford the general forms, so this module keeps
//! the shapes that embedded products actually use:
//!
//! | Primitive | Linux analogue | Use |
//! |---|---|---|
//! | [`MessageQueue`] | `mq_*` / kfifo | ISR → task events, command batches |
//! | [`Mailbox`] | `eventfd`-like single slot | "latest reading" / "latest command" |
//! | [`Pipe`] | `pipe(2)` | console input, log/byte streams |
//! | [`Semaphore`] | `sem_*` | counting resource tokens |
//!
//! Blocking waits are deliberately absent: a wait needs a waker and a critical
//! section from the arch layer. These types are the *state*; the executor layer
//! will compose them with wakers (`Signal` already exists for wakeups).
//!
//! All primitives are `&mut`-accessed and allocation-free, so their behaviour is
//! unit-tested on the host exactly as it runs on target.

use core::sync::atomic::{AtomicU32, Ordering};

/// A bounded FIFO of `T` with capacity `N`.
#[derive(Clone)]
pub struct MessageQueue<T: Copy + Default, const N: usize> {
    items: [T; N],
    head: usize,
    len: usize,
    high_water: usize,
}

impl<T: Copy + Default, const N: usize> Default for MessageQueue<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default, const N: usize> MessageQueue<T, N> {
    /// Create an empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self {
            items: [T::default(); N],
            head: 0,
            len: 0,
            high_water: 0,
        }
    }

    /// Number of queued items.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the queue is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether the queue is full.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.len == N
    }

    /// Highest occupancy seen since the last [`MessageQueue::reset_high_water`].
    #[must_use]
    pub const fn high_water(&self) -> usize {
        self.high_water
    }

    /// Clear the high-water mark.
    pub fn reset_high_water(&mut self) {
        self.high_water = self.len;
    }

    /// Push an item, returning it back when the queue is full.
    pub fn push(&mut self, item: T) -> Result<(), T> {
        if N == 0 || self.len == N {
            return Err(item);
        }
        let index = (self.head + self.len) % N;
        self.items[index] = item;
        self.len += 1;
        self.high_water = self.high_water.max(self.len);
        Ok(())
    }

    /// Push an item, dropping the oldest when full.
    pub fn push_overwrite(&mut self, item: T) -> Option<T> {
        if N == 0 {
            return Some(item);
        }
        if self.len == N {
            let dropped = self.items[self.head];
            self.items[self.head] = item;
            self.head = (self.head + 1) % N;
            return Some(dropped);
        }
        let _ = self.push(item);
        None
    }

    /// Remove the oldest item.
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let item = self.items[self.head];
        self.head = (self.head + 1) % N;
        self.len -= 1;
        Some(item)
    }

    /// Look at the oldest item without removing it.
    #[must_use]
    pub fn peek(&self) -> Option<&T> {
        if self.len == 0 {
            None
        } else {
            Some(&self.items[self.head])
        }
    }

    /// Drop every item.
    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }
}

/// A single-slot mailbox that keeps the most recent value.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mailbox<T: Copy> {
    slot: Option<T>,
    overwrites: u32,
}

impl<T: Copy> Mailbox<T> {
    /// Create an empty mailbox.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slot: None,
            overwrites: 0,
        }
    }

    /// Store a value, returning the previous one and counting the overwrite.
    pub fn set(&mut self, value: T) -> Option<T> {
        let previous = self.slot.replace(value);
        if previous.is_some() {
            self.overwrites = self.overwrites.saturating_add(1);
        }
        previous
    }

    /// Take the value out.
    pub fn take(&mut self) -> Option<T> {
        self.slot.take()
    }

    /// Borrow the value.
    #[must_use]
    pub const fn peek(&self) -> Option<&T> {
        self.slot.as_ref()
    }

    /// Whether the mailbox is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.slot.is_none()
    }

    /// Number of values replaced before being taken.
    #[must_use]
    pub const fn overwrites(&self) -> u32 {
        self.overwrites
    }
}

/// A byte stream with fixed capacity, like a `pipe(2)` with a bounded buffer.
#[derive(Clone)]
pub struct Pipe<const N: usize> {
    bytes: [u8; N],
    head: usize,
    len: usize,
    high_water: usize,
}

impl<const N: usize> Default for Pipe<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Pipe<N> {
    /// Create an empty pipe.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: [0; N],
            head: 0,
            len: 0,
            high_water: 0,
        }
    }

    /// Bytes buffered.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the pipe is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bytes that can still be written.
    #[must_use]
    pub const fn free(&self) -> usize {
        N - self.len
    }

    /// Highest occupancy seen since [`Pipe::reset_high_water`].
    #[must_use]
    pub const fn high_water(&self) -> usize {
        self.high_water
    }

    /// Clear the high-water mark.
    pub fn reset_high_water(&mut self) {
        self.high_water = self.len;
    }

    /// Write as much of `data` as fits; returns the byte count written.
    pub fn write(&mut self, data: &[u8]) -> usize {
        let count = data.len().min(self.free());
        for (offset, byte) in data[..count].iter().enumerate() {
            let index = (self.head + self.len + offset) % N;
            self.bytes[index] = *byte;
        }
        self.len += count;
        self.high_water = self.high_water.max(self.len);
        count
    }

    /// Read up to `out.len()` bytes; returns the byte count read.
    pub fn read(&mut self, out: &mut [u8]) -> usize {
        let count = out.len().min(self.len);
        for (offset, slot) in out[..count].iter_mut().enumerate() {
            let index = (self.head + offset) % N;
            *slot = self.bytes[index];
        }
        self.head = (self.head + count) % N;
        self.len -= count;
        count
    }

    /// Drop buffered bytes.
    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }
}

/// A counting semaphore with non-blocking operations.
///
/// A blocking `wait` needs a waker; the executor layer composes one with a
/// [`crate::sync::Signal`].
#[derive(Debug)]
pub struct Semaphore {
    count: AtomicU32,
}

impl Semaphore {
    /// Create a semaphore with `count` tokens.
    #[must_use]
    pub const fn new(count: u32) -> Self {
        Self {
            count: AtomicU32::new(count),
        }
    }

    /// Take a token if one is available.
    pub fn try_wait(&self) -> bool {
        let mut current = self.count.load(Ordering::Acquire);
        loop {
            if current == 0 {
                return false;
            }
            match self.count.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }

    /// Add one token (saturating).
    pub fn signal(&self) {
        let mut current = self.count.load(Ordering::Acquire);
        loop {
            let next = current.saturating_add(1);
            if next == current {
                return;
            }
            match self.count.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    /// Current token count.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.count.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_is_fifo() {
        let mut queue = MessageQueue::<u32, 4>::new();
        queue.push(1).unwrap();
        queue.push(2).unwrap();
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn queue_wraps_and_tracks_high_water() {
        let mut queue = MessageQueue::<u32, 3>::new();
        for value in 0..3 {
            queue.push(value).unwrap();
        }
        assert!(queue.is_full());
        assert_eq!(queue.push(9), Err(9));
        assert_eq!(queue.pop(), Some(0));
        queue.push(3).unwrap();
        assert_eq!(queue.high_water(), 3);
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
    }

    #[test]
    fn queue_overwrite_drops_the_oldest() {
        let mut queue = MessageQueue::<u32, 2>::new();
        queue.push(1).unwrap();
        queue.push(2).unwrap();
        assert_eq!(queue.push_overwrite(3), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
    }

    #[test]
    fn zero_capacity_queue_is_safe() {
        let mut queue = MessageQueue::<u32, 0>::new();
        assert!(queue.is_full());
        assert_eq!(queue.push(1), Err(1));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn mailbox_keeps_latest() {
        let mut mailbox = Mailbox::new();
        assert!(mailbox.is_empty());
        assert_eq!(mailbox.set(10), None);
        assert_eq!(mailbox.set(11), Some(10));
        assert_eq!(mailbox.overwrites(), 1);
        assert_eq!(mailbox.peek(), Some(&11));
        assert_eq!(mailbox.take(), Some(11));
        assert!(mailbox.is_empty());
    }

    #[test]
    fn pipe_wraps_and_reports_partial_io() {
        let mut pipe = Pipe::<4>::new();
        assert_eq!(pipe.write(b"abc"), 3);
        assert_eq!(pipe.write(b"def"), 1);
        assert_eq!(pipe.len(), 4);
        assert_eq!(pipe.free(), 0);

        let mut out = [0u8; 3];
        assert_eq!(pipe.read(&mut out), 3);
        assert_eq!(&out, b"abc");
        assert_eq!(pipe.write(b"de"), 2);
        assert_eq!(pipe.high_water(), 4);

        let mut rest = [0u8; 8];
        assert_eq!(pipe.read(&mut rest), 3);
        assert_eq!(&rest[..3], b"dde");
    }

    #[test]
    fn semaphore_counts_and_saturates() {
        let semaphore = Semaphore::new(2);
        assert_eq!(semaphore.count(), 2);
        assert!(semaphore.try_wait());
        assert!(semaphore.try_wait());
        assert!(!semaphore.try_wait());
        semaphore.signal();
        semaphore.signal();
        semaphore.signal();
        assert_eq!(semaphore.count(), 3);

        let full = Semaphore::new(u32::MAX);
        full.signal();
        assert_eq!(full.count(), u32::MAX);
    }

    #[test]
    fn semaphore_is_visible_across_threads() {
        let semaphore = Semaphore::new(0);
        std::thread::scope(|scope| {
            scope.spawn(|| semaphore.signal());
        });
        assert!(semaphore.try_wait());
    }
}
