//! Deterministic scheduler core.
//!
//! Scheduling model (ADR-002): tasks are async futures grouped into four
//! priority classes. One executor per class; within a class, round-robin over
//! a ready bitmask. The scheduler is cooperative — [`Scheduler::next_ready`] is
//! called when the executor is ready to poll a task, and a task yields at
//! `.await`.
//!
//! All state is statically sized: a fixed task table ([`MAX_TASKS`]) and a
//! fixed timer heap ([`MAX_TIMERS`]). There is no allocation and no unsafe.
//!
//! The scheduler does not read a clock. The caller advances time with
//! [`Scheduler::advance_to`], which wakes every task whose deadline is due.

/// Maximum number of schedulable tasks.
pub const MAX_TASKS: usize = crate::config::MAX_TASKS;

/// Maximum number of pending timers (including stale ones awaiting expiry).
pub const MAX_TIMERS: usize = crate::config::MAX_TIMERS;

/// Errors returned by the scheduler core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedError {
    /// Task id is outside the task table, or the task is not registered.
    InvalidTask,
    /// The timer heap is full; the task was not armed.
    TimerQueueFull,
    /// `end_run` was called without a matching `begin_run`.
    NotRunning,
}

/// Per-task scheduling accounting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskStats {
    /// Times the task was made ready by a wake (not by a timer).
    pub wakes: u32,
    /// Completed run measurements.
    pub runs: u32,
    /// Total measured run time, in microseconds.
    pub run_us: u64,
    /// Longest single run, in microseconds.
    pub max_run_us: u64,
}

impl TaskStats {
    /// Zeroed statistics.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            wakes: 0,
            runs: 0,
            run_us: 0,
            max_run_us: 0,
        }
    }

    /// Mean run time in microseconds (0 before the first run).
    #[must_use]
    pub const fn mean_run_us(&self) -> u64 {
        if self.runs == 0 {
            0
        } else {
            self.run_us / self.runs as u64
        }
    }
}

/// Priority class of a task.
///
/// Ordering is the dispatch order: `Ctrl` always runs before `App`, which runs
/// before `Net`. `Idle` only runs when nothing else is ready.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Control loops, input handling, hard deadlines.
    Ctrl = 0,
    /// Application state machines, sensors, storage.
    App = 1,
    /// Networking housekeeping, MQTT, telemetry.
    Net = 2,
    /// Idle work; the sleep policy hangs off this class.
    Idle = 3,
}

impl Class {
    /// Number of classes.
    pub const COUNT: usize = 4;

    /// Classes in dispatch order.
    pub const ALL: [Self; Self::COUNT] = [Self::Ctrl, Self::App, Self::Net, Self::Idle];

    /// Index into per-class arrays.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Identifier of a task, in `0..MAX_TASKS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(u8);

impl TaskId {
    /// Create an id from its raw value.
    ///
    /// Values at or above [`MAX_TASKS`] are rejected by the scheduler, not
    /// here, so that a board manifest can be validated at build time.
    #[must_use]
    pub const fn new(raw: u8) -> Self {
        Self(raw)
    }

    /// Raw value, e.g. for logging.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }

    const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A FIFO/round-robin ready queue backed by a bitmask.
///
/// `pop` starts at the bit after the one it returned last time, so repeated
/// calls cycle through the ready set instead of starving the low ids.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReadyQueue {
    mask: u32,
    cursor: u8,
}

impl ReadyQueue {
    /// Create an empty queue.
    #[must_use]
    pub const fn new() -> Self {
        Self { mask: 0, cursor: 0 }
    }

    /// Whether the queue holds no task.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.mask == 0
    }

    /// Number of ready tasks.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.mask.count_ones()
    }

    /// Whether `task` is currently ready.
    #[must_use]
    pub const fn contains(&self, task: TaskId) -> bool {
        task.index() < MAX_TASKS && (self.mask & (1u32 << task.index())) != 0
    }

    /// Mark `task` ready. Waking an already-ready task is a no-op.
    pub fn push(&mut self, task: TaskId) -> Result<(), SchedError> {
        if task.index() >= MAX_TASKS {
            return Err(SchedError::InvalidTask);
        }
        self.mask |= 1u32 << task.index();
        Ok(())
    }

    /// Remove `task` from the queue if present.
    pub fn remove(&mut self, task: TaskId) {
        if task.index() < MAX_TASKS {
            self.mask &= !(1u32 << task.index());
        }
    }

    /// Take the next ready task, round-robin.
    pub fn pop(&mut self) -> Option<TaskId> {
        if self.mask == 0 {
            return None;
        }
        let cursor = u32::from(self.cursor) % 32;
        let rotated = self.mask.rotate_right(cursor);
        let offset = rotated.trailing_zeros();
        let index = (cursor + offset) % 32;
        self.mask &= !(1u32 << index);
        self.cursor = ((index + 1) % 32) as u8;
        Some(TaskId::new(index as u8))
    }
}

/// One armed deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timer {
    /// Monotonic deadline in microseconds.
    pub deadline_us: u64,
    /// Task to wake.
    pub task: TaskId,
    /// Sleep generation; a timer is stale once the task has slept again or
    /// been woken by another source.
    pub generation: u32,
}

const EMPTY_TIMER: Timer = Timer {
    deadline_us: u64::MAX,
    task: TaskId::new(0),
    generation: 0,
};

/// Fixed-capacity binary min-heap of timers, ordered by deadline.
#[derive(Clone, Copy, Debug)]
pub struct TimerHeap {
    items: [Timer; MAX_TIMERS],
    len: usize,
}

impl Default for TimerHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerHeap {
    /// Create an empty heap.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            items: [EMPTY_TIMER; MAX_TIMERS],
            len: 0,
        }
    }

    /// Number of armed timers, including stale ones.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no timer is armed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Arm a timer. Fails when the heap is full.
    pub fn push(&mut self, timer: Timer) -> Result<(), SchedError> {
        if self.len == MAX_TIMERS {
            return Err(SchedError::TimerQueueFull);
        }
        let mut index = self.len;
        self.items[index] = timer;
        self.len += 1;
        while index > 0 {
            let parent = (index - 1) / 2;
            if self.items[parent].deadline_us <= self.items[index].deadline_us {
                break;
            }
            self.items.swap(parent, index);
            index = parent;
        }
        Ok(())
    }

    /// The earliest deadline, without removing it.
    #[must_use]
    pub fn peek(&self) -> Option<&Timer> {
        (self.len != 0).then(|| &self.items[0])
    }

    /// Remove and return the earliest timer.
    pub fn pop(&mut self) -> Option<Timer> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        self.items.swap(0, self.len);
        let timer = self.items[self.len];
        self.sift_down(0);
        Some(timer)
    }

    /// Drop every armed timer.
    pub fn clear(&mut self) {
        self.len = 0;
    }

    fn sift_down(&mut self, mut index: usize) {
        loop {
            let left = 2 * index + 1;
            if left >= self.len {
                break;
            }
            let right = left + 1;
            let mut smallest = left;
            if right < self.len && self.items[right].deadline_us < self.items[left].deadline_us {
                smallest = right;
            }
            if self.items[index].deadline_us <= self.items[smallest].deadline_us {
                break;
            }
            self.items.swap(index, smallest);
            index = smallest;
        }
    }
}

/// Priority scheduler: ready queues per class plus a deadline heap.
#[derive(Clone, Debug)]
pub struct Scheduler {
    ready: [ReadyQueue; Class::COUNT],
    timers: TimerHeap,
    classes: [Option<Class>; MAX_TASKS],
    suspended: [bool; MAX_TASKS],
    promoted: [bool; MAX_TASKS],
    ready_since: [u64; MAX_TASKS],
    run_started: [Option<u64>; MAX_TASKS],
    stats: [TaskStats; MAX_TASKS],
    generations: [u32; MAX_TASKS],
    now_us: u64,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    /// Create an empty scheduler with no registered tasks.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ready: [ReadyQueue::new(); Class::COUNT],
            timers: TimerHeap::new(),
            classes: [None; MAX_TASKS],
            suspended: [false; MAX_TASKS],
            promoted: [false; MAX_TASKS],
            ready_since: [0; MAX_TASKS],
            run_started: [None; MAX_TASKS],
            stats: [TaskStats::new(); MAX_TASKS],
            generations: [0; MAX_TASKS],
            now_us: 0,
        }
    }

    /// Current monotonic time as last advanced by the caller.
    #[must_use]
    pub const fn now_us(&self) -> u64 {
        self.now_us
    }

    /// Register a task in a class. Registration is boot-time only.
    pub fn register(&mut self, task: TaskId, class: Class) -> Result<(), SchedError> {
        if task.index() >= MAX_TASKS {
            return Err(SchedError::InvalidTask);
        }
        self.classes[task.index()] = Some(class);
        Ok(())
    }

    /// The class of a registered task.
    #[must_use]
    pub fn class_of(&self, task: TaskId) -> Option<Class> {
        if task.index() >= MAX_TASKS {
            return None;
        }
        self.classes[task.index()]
    }

    /// Whether `task` is ready to run (including a promoted task).
    #[must_use]
    pub fn is_ready(&self, task: TaskId) -> bool {
        self.queued_class(task).is_some()
    }

    /// Class queue currently holding the task, which differs from its base
    /// class while it is promoted by aging.
    #[must_use]
    pub fn queued_class(&self, task: TaskId) -> Option<Class> {
        Class::ALL
            .into_iter()
            .find(|class| self.ready[class.index()].contains(task))
    }

    /// Accounting for a task.
    #[must_use]
    pub fn stats(&self, task: TaskId) -> Option<&TaskStats> {
        if task.index() >= MAX_TASKS || self.class_of(task).is_none() {
            return None;
        }
        Some(&self.stats[task.index()])
    }

    /// Start measuring a run. Pair with [`Scheduler::end_run`].
    pub fn begin_run(&mut self, task: TaskId, now_us: u64) -> Result<(), SchedError> {
        if self.class_of(task).is_none() {
            return Err(SchedError::InvalidTask);
        }
        self.run_started[task.index()] = Some(now_us);
        Ok(())
    }

    /// Finish measuring a run, updating the task's stats. Returns the duration.
    pub fn end_run(&mut self, task: TaskId, now_us: u64) -> Result<u64, SchedError> {
        if self.class_of(task).is_none() {
            return Err(SchedError::InvalidTask);
        }
        let Some(started) = self.run_started[task.index()].take() else {
            return Err(SchedError::NotRunning);
        };
        let duration = now_us.saturating_sub(started);
        let stats = &mut self.stats[task.index()];
        stats.runs = stats.runs.saturating_add(1);
        stats.run_us = stats.run_us.saturating_add(duration);
        stats.max_run_us = stats.max_run_us.max(duration);
        Ok(duration)
    }

    /// Promote ready tasks that have waited longer than
    /// [`crate::config::AGING_THRESHOLD_US`], one class at a time.
    ///
    /// Returns how many tasks were promoted. The executor calls this before
    /// choosing the next task; a promoted task is returned to its base class
    /// after it runs.
    pub fn age(&mut self, now_us: u64) -> u32 {
        let mut promoted = 0;
        for raw in 0..MAX_TASKS {
            let Some(class) = self.classes[raw] else {
                continue;
            };
            if class == Class::Ctrl || self.promoted[raw] {
                continue;
            }
            let task = TaskId::new(raw as u8);
            if !self.ready[class.index()].contains(task) {
                continue;
            }
            if now_us.saturating_sub(self.ready_since[raw]) < crate::config::AGING_THRESHOLD_US {
                continue;
            }
            let target = Class::ALL[class.index() - 1];
            self.ready[class.index()].remove(task);
            if self.ready[target.index()].push(task).is_ok() {
                self.promoted[raw] = true;
                promoted += 1;
            }
        }
        promoted
    }

    /// Wake a task: invalidate any pending sleep and mark it ready.
    ///
    /// Waking an already-ready task is a no-op at the queue level; the
    /// generation bump cancels a stale timer that would otherwise fire later.
    /// A suspended task swallows the wake: it becomes ready only on
    /// [`Scheduler::resume`].
    pub fn wake(&mut self, task: TaskId) -> Result<(), SchedError> {
        let Some(class) = self.class_of(task) else {
            return Err(SchedError::InvalidTask);
        };
        self.generations[task.index()] = self.generations[task.index()].wrapping_add(1);
        if self.suspended[task.index()] {
            return Ok(());
        }
        if self.is_ready(task) {
            return Ok(());
        }
        self.ready[class.index()].push(task)?;
        self.ready_since[task.index()] = self.now_us;
        self.stats[task.index()].wakes = self.stats[task.index()].wakes.saturating_add(1);
        Ok(())
    }

    /// Suspend a task: remove it from the ready set and ignore wakes until
    /// [`Scheduler::resume`].
    pub fn suspend(&mut self, task: TaskId) -> Result<(), SchedError> {
        let Some(class) = self.class_of(task) else {
            return Err(SchedError::InvalidTask);
        };
        self.suspended[task.index()] = true;
        self.promoted[task.index()] = false;
        if let Some(queued) = self.queued_class(task) {
            self.ready[queued.index()].remove(task);
        }
        self.ready[class.index()].remove(task);
        Ok(())
    }

    /// Resume a suspended task, making it ready.
    pub fn resume(&mut self, task: TaskId) -> Result<(), SchedError> {
        let Some(class) = self.class_of(task) else {
            return Err(SchedError::InvalidTask);
        };
        self.suspended[task.index()] = false;
        self.promoted[task.index()] = false;
        self.generations[task.index()] = self.generations[task.index()].wrapping_add(1);
        if self.is_ready(task) {
            return Ok(());
        }
        self.ready[class.index()].push(task)?;
        self.ready_since[task.index()] = self.now_us;
        Ok(())
    }

    /// Whether a task is suspended.
    #[must_use]
    pub fn is_suspended(&self, task: TaskId) -> bool {
        task.index() < MAX_TASKS && self.suspended[task.index()]
    }

    /// Arm a deadline for a sleeping task.
    pub fn sleep_until(&mut self, task: TaskId, deadline_us: u64) -> Result<(), SchedError> {
        if self.class_of(task).is_none() {
            return Err(SchedError::InvalidTask);
        }
        let generation = self.generations[task.index()].wrapping_add(1);
        self.timers.push(Timer {
            deadline_us,
            task,
            generation,
        })?;
        self.generations[task.index()] = generation;
        Ok(())
    }

    /// Arm a relative deadline, saturating on overflow.
    pub fn sleep_for(&mut self, task: TaskId, delta_us: u64) -> Result<(), SchedError> {
        self.sleep_until(task, self.now_us.saturating_add(delta_us))
    }

    /// Pick the highest-priority ready task, round-robin inside its class.
    ///
    /// The task is removed from the ready set; the executor marks it ready
    /// again when its future returns `Pending`.
    pub fn next_ready(&mut self) -> Option<TaskId> {
        for class in Class::ALL {
            if let Some(task) = self.ready[class.index()].pop() {
                self.promoted[task.index()] = false;
                return Some(task);
            }
        }
        None
    }

    /// Advance time to `now_us` and wake every task whose deadline is due.
    ///
    /// Returns the number of tasks woken. Time never moves backwards: a
    /// `now_us` below the current time is ignored.
    pub fn advance_to(&mut self, now_us: u64) -> usize {
        if now_us > self.now_us {
            self.now_us = now_us;
        }
        let mut woken = 0;
        while let Some(timer) = self.timers.peek() {
            if timer.deadline_us > self.now_us {
                break;
            }
            let timer = self.timers.pop().expect("peek returned a timer");
            let index = timer.task.index();
            if index < MAX_TASKS
                && timer.generation == self.generations[index]
                && !self.suspended[index]
                && self.wake(timer.task).is_ok()
            {
                woken += 1;
            }
        }
        woken
    }

    /// Earliest armed deadline, for programming the hardware comparator.
    #[must_use]
    pub fn next_deadline(&self) -> Option<u64> {
        self.timers.peek().map(|timer| timer.deadline_us)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(raw: u8) -> TaskId {
        TaskId::new(raw)
    }

    #[test]
    fn rejects_unregistered_task() {
        let mut scheduler = Scheduler::new();
        assert_eq!(scheduler.wake(task(1)), Err(SchedError::InvalidTask));
        assert_eq!(
            scheduler.sleep_until(task(1), 10),
            Err(SchedError::InvalidTask)
        );
        assert_eq!(scheduler.wake(task(200)), Err(SchedError::InvalidTask));
        assert_eq!(scheduler.class_of(task(1)), None);
    }

    #[test]
    fn class_order_is_ctrl_app_net_idle() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::Net).unwrap();
        scheduler.register(task(2), Class::App).unwrap();
        scheduler.register(task(3), Class::Ctrl).unwrap();
        scheduler.register(task(4), Class::Idle).unwrap();

        // Wake in reverse priority order on purpose.
        scheduler.wake(task(1)).unwrap();
        scheduler.wake(task(4)).unwrap();
        scheduler.wake(task(2)).unwrap();
        scheduler.wake(task(3)).unwrap();

        assert_eq!(scheduler.next_ready(), Some(task(3)));
        assert_eq!(scheduler.next_ready(), Some(task(2)));
        assert_eq!(scheduler.next_ready(), Some(task(1)));
        assert_eq!(scheduler.next_ready(), Some(task(4)));
        assert_eq!(scheduler.next_ready(), None);
    }

    #[test]
    fn round_robin_within_a_class() {
        let mut scheduler = Scheduler::new();
        for raw in 1..=3 {
            scheduler.register(task(raw), Class::App).unwrap();
        }
        for _round in 0..3 {
            for raw in 1..=3 {
                scheduler.wake(task(raw)).unwrap();
            }
            for raw in 1..=3 {
                assert_eq!(scheduler.next_ready(), Some(task(raw)));
            }
        }
    }

    #[test]
    fn duplicate_wake_enqueues_once() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::Ctrl).unwrap();
        scheduler.wake(task(1)).unwrap();
        scheduler.wake(task(1)).unwrap();
        assert!(scheduler.is_ready(task(1)));
        assert_eq!(scheduler.next_ready(), Some(task(1)));
        assert_eq!(scheduler.next_ready(), None);
    }

    #[test]
    fn timers_fire_in_deadline_order() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::App).unwrap();
        scheduler.register(task(2), Class::App).unwrap();
        scheduler.sleep_until(task(1), 100).unwrap();
        scheduler.sleep_until(task(2), 50).unwrap();
        assert_eq!(scheduler.next_deadline(), Some(50));

        assert_eq!(scheduler.advance_to(0), 0);
        assert_eq!(scheduler.next_ready(), None);

        assert_eq!(scheduler.advance_to(60), 1);
        assert_eq!(scheduler.next_ready(), Some(task(2)));

        assert_eq!(scheduler.advance_to(100), 1);
        assert_eq!(scheduler.next_ready(), Some(task(1)));
        assert_eq!(scheduler.next_deadline(), None);
    }

    #[test]
    fn early_wake_cancels_pending_timer() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::Net).unwrap();
        scheduler.sleep_until(task(1), 100).unwrap();

        // An event wakes the task before its deadline.
        scheduler.wake(task(1)).unwrap();
        assert_eq!(scheduler.next_ready(), Some(task(1)));

        // The stale timer must not enqueue the task again.
        assert_eq!(scheduler.advance_to(100), 0);
        assert_eq!(scheduler.next_ready(), None);
    }

    #[test]
    fn resleep_invalidates_previous_deadline() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::App).unwrap();

        scheduler.sleep_until(task(1), 100).unwrap();
        scheduler.sleep_until(task(1), 200).unwrap();

        assert_eq!(scheduler.advance_to(100), 0);
        assert_eq!(scheduler.next_ready(), None);
        assert_eq!(scheduler.advance_to(200), 1);
        assert_eq!(scheduler.next_ready(), Some(task(1)));
    }

    #[test]
    fn timer_heap_reports_full() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::App).unwrap();
        for _ in 0..MAX_TIMERS {
            scheduler.sleep_until(task(1), 1_000).unwrap();
        }
        assert_eq!(
            scheduler.sleep_until(task(1), 1_000),
            Err(SchedError::TimerQueueFull)
        );
    }

    #[test]
    fn time_never_moves_backwards() {
        let mut scheduler = Scheduler::new();
        assert_eq!(scheduler.advance_to(500), 0);
        assert_eq!(scheduler.now_us(), 500);
        assert_eq!(scheduler.advance_to(100), 0);
        assert_eq!(scheduler.now_us(), 500);
    }

    #[test]
    fn suspend_blocks_wake_until_resume() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::App).unwrap();

        scheduler.wake(task(1)).unwrap();
        scheduler.suspend(task(1)).unwrap();
        assert!(scheduler.is_suspended(task(1)));
        assert!(!scheduler.is_ready(task(1)));

        assert!(scheduler.wake(task(1)).is_ok());
        assert!(!scheduler.is_ready(task(1)));
        assert_eq!(scheduler.next_ready(), None);

        scheduler.resume(task(1)).unwrap();
        assert!(!scheduler.is_suspended(task(1)));
        assert_eq!(scheduler.next_ready(), Some(task(1)));
    }

    #[test]
    fn suspended_timer_does_not_wake() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::App).unwrap();
        scheduler.suspend(task(1)).unwrap();
        scheduler.sleep_until(task(1), 100).unwrap();

        assert_eq!(scheduler.advance_to(100), 0);
        assert_eq!(scheduler.next_ready(), None);
    }

    #[test]
    fn accounting_accumulates_runs() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::App).unwrap();

        scheduler.begin_run(task(1), 100).unwrap();
        assert_eq!(scheduler.end_run(task(1), 350), Ok(250));
        scheduler.begin_run(task(1), 400).unwrap();
        assert_eq!(scheduler.end_run(task(1), 450), Ok(50));

        let stats = scheduler.stats(task(1)).unwrap();
        assert_eq!(stats.runs, 2);
        assert_eq!(stats.run_us, 300);
        assert_eq!(stats.max_run_us, 250);
        assert_eq!(stats.mean_run_us(), 150);
    }

    #[test]
    fn end_run_requires_a_start() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::App).unwrap();
        assert_eq!(scheduler.end_run(task(1), 10), Err(SchedError::NotRunning));
        assert_eq!(
            scheduler.begin_run(task(9), 0),
            Err(SchedError::InvalidTask)
        );
    }

    #[test]
    fn wakes_are_counted_only_when_queued() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::App).unwrap();
        scheduler.wake(task(1)).unwrap();
        scheduler.wake(task(1)).unwrap();
        assert_eq!(scheduler.stats(task(1)).unwrap().wakes, 1);
        assert_eq!(scheduler.next_ready(), Some(task(1)));
        scheduler.wake(task(1)).unwrap();
        assert_eq!(scheduler.stats(task(1)).unwrap().wakes, 2);
    }

    #[test]
    fn aging_promotes_a_starved_task() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::Net).unwrap();
        scheduler.wake(task(1)).unwrap();
        assert_eq!(scheduler.age(0), 0);

        let now = crate::config::AGING_THRESHOLD_US + 1;
        scheduler.advance_to(now);
        assert_eq!(scheduler.age(now), 1);
        assert_eq!(scheduler.queued_class(task(1)), Some(Class::App));

        assert_eq!(scheduler.next_ready(), Some(task(1)));
        assert_eq!(scheduler.queued_class(task(1)), None);
    }

    #[test]
    fn control_class_is_never_promoted() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::Ctrl).unwrap();
        scheduler.wake(task(1)).unwrap();
        let now = crate::config::AGING_THRESHOLD_US * 4;
        scheduler.advance_to(now);
        assert_eq!(scheduler.age(now), 0);
        assert_eq!(scheduler.queued_class(task(1)), Some(Class::Ctrl));
    }

    #[test]
    fn a_promoted_task_is_not_duplicated_by_a_wake() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::Net).unwrap();
        scheduler.wake(task(1)).unwrap();
        let now = crate::config::AGING_THRESHOLD_US + 1;
        scheduler.advance_to(now);
        scheduler.age(now);

        scheduler.wake(task(1)).unwrap();
        assert!(scheduler.is_ready(task(1)));
        assert_eq!(scheduler.next_ready(), Some(task(1)));
        assert_eq!(scheduler.next_ready(), None);
    }

    #[test]
    fn relative_sleep_saturates() {
        let mut scheduler = Scheduler::new();
        scheduler.register(task(1), Class::App).unwrap();
        scheduler.advance_to(10);
        scheduler.sleep_for(task(1), u64::MAX).unwrap();
        assert_eq!(scheduler.next_deadline(), Some(u64::MAX));
    }
}
