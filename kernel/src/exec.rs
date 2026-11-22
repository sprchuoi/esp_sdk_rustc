//! Cooperative executor skeleton.
//!
//! This is the policy half of the executor: static task slots, class
//! registration, `spawn`/`spawn_periodic`, cooperative `yield`, and accounting
//! through [`Scheduler`]. Task bodies are plain `fn(&mut TaskCtx) -> Step`, so
//! the whole loop is host-testable.
//!
//! The stack half (one 8 KiB stack per class, context switch, waker) is the next
//! layer; it replaces `Step` with a polled future without changing this API.

use crate::sched::{Class, SchedError, Scheduler, TaskId};

/// What a cooperative task did in this step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Work remains; run again on the next pass.
    Yield,
    /// Nothing to do until the next wake/deadline.
    Idle,
}

/// Per-step task context.
pub struct TaskCtx {
    /// This task's id.
    pub task: TaskId,
    /// Current monotonic time in microseconds.
    pub now_us: u64,
}

/// A cooperative task body.
pub type TaskBody = fn(&mut TaskCtx) -> Step;

struct Slot {
    body: TaskBody,
    period_us: u64,
}

/// Static executor over [`Scheduler`].
pub struct Executor<const N: usize> {
    scheduler: Scheduler,
    slots: [Option<Slot>; N],
    steps: u32,
}

impl<const N: usize> Default for Executor<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Executor<N> {
    /// Create an executor with an empty scheduler.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::new(),
            slots: core::array::from_fn(|_| None),
            steps: 0,
        }
    }

    /// Spawn a one-shot task in `class`.
    ///
    /// # Errors
    /// Scheduler registration errors, or a full executor.
    pub fn spawn(&mut self, body: TaskBody, class: Class) -> Result<TaskId, SchedError> {
        let raw = self
            .slots
            .iter()
            .position(Option::is_none)
            .ok_or(SchedError::TaskSlotsFull)?;
        let task = TaskId::new(raw as u8);
        self.scheduler.register(task, class)?;
        self.slots[raw] = Some(Slot { body, period_us: 0 });
        self.scheduler.wake(task)?;
        Ok(task)
    }

    /// Spawn a periodic task; it is re-armed after every run.
    ///
    /// # Errors
    /// As [`Executor::spawn`], plus [`SchedError::InvalidPeriod`].
    pub fn spawn_periodic(
        &mut self,
        body: TaskBody,
        class: Class,
        period_us: u64,
        now_us: u64,
    ) -> Result<TaskId, SchedError> {
        let raw = self
            .slots
            .iter()
            .position(Option::is_none)
            .ok_or(SchedError::TaskSlotsFull)?;
        let task = TaskId::new(raw as u8);
        self.scheduler.register(task, class)?;
        self.slots[raw] = Some(Slot { body, period_us });
        self.scheduler.arm_periodic(task, period_us, now_us)?;
        Ok(task)
    }

    /// Run every currently ready task once, in class order.
    ///
    /// Returns the number of steps executed.
    pub fn run_once(&mut self, now_us: u64) -> u32 {
        self.scheduler.advance_to(now_us);
        self.scheduler.age(now_us);

        let mut ran = 0;
        let mut yielded = 0u64;
        while let Some(task) = self.scheduler.next_ready() {
            let raw = task.index();
            let Some(slot) = self.slots[raw].as_mut() else {
                continue;
            };
            let mut ctx = TaskCtx { task, now_us };
            self.scheduler.begin_run(task, now_us).ok();
            let step = (slot.body)(&mut ctx);
            self.scheduler.end_run(task, now_us).ok();
            self.steps += 1;
            ran += 1;

            match (slot.period_us, step) {
                (period, _) if period > 0 => {
                    let _ = self.scheduler.arm_periodic(task, period, now_us);
                }
                (_, Step::Yield) => yielded |= 1 << raw,
                (_, Step::Idle) => {}
            }
        }
        for raw in 0..N {
            if yielded & (1 << raw) != 0 {
                let _ = self.scheduler.wake(TaskId::new(raw as u8));
            }
        }
        ran
    }

    /// Read-only scheduler access.
    #[must_use]
    pub const fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Mutable scheduler access (suspend/resume from a service).
    pub const fn scheduler_mut(&mut self) -> &mut Scheduler {
        &mut self.scheduler
    }

    /// Total steps executed.
    #[must_use]
    pub const fn steps(&self) -> u32 {
        self.steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};

    static ORDER: AtomicU32 = AtomicU32::new(0);
    static MASK: AtomicU32 = AtomicU32::new(0);

    fn low(_: &mut TaskCtx) -> Step {
        MASK.fetch_or(1, Ordering::Relaxed);
        Step::Idle
    }

    fn high(_: &mut TaskCtx) -> Step {
        MASK.fetch_or(2, Ordering::Relaxed);
        Step::Idle
    }

    fn yielder(ctx: &mut TaskCtx) -> Step {
        ORDER.fetch_add(1, Ordering::Relaxed);
        let _ = ctx;
        Step::Yield
    }

    #[test]
    fn runs_in_class_order() {
        let mut exec = Executor::<4>::new();
        MASK.store(0, Ordering::Relaxed);
        exec.spawn(low, Class::Net).unwrap();
        exec.spawn(high, Class::Ctrl).unwrap();
        assert_eq!(exec.run_once(0), 2);
        assert_eq!(MASK.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn yield_runs_again_immediately() {
        let mut exec = Executor::<2>::new();
        ORDER.store(0, Ordering::Relaxed);
        exec.spawn(yielder, Class::App).unwrap();
        assert_eq!(exec.run_once(0), 1);
        assert_eq!(exec.run_once(0), 1);
        assert_eq!(ORDER.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn periodic_task_rearms() {
        let mut exec = Executor::<2>::new();
        exec.spawn_periodic(high, Class::App, 100, 0).unwrap();
        assert_eq!(exec.run_once(0), 0); // first deadline at 100
        assert_eq!(exec.run_once(100), 1);
        assert_eq!(exec.run_once(200), 1);
        assert!(exec.scheduler().stats(TaskId::new(0)).unwrap().runs >= 2);
    }
}
