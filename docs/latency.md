# Interrupt and wake latency

**Status:** method defined · hardware measurement pending
Phase 1 requires numbers, not assumptions. This document is the procedure, the
targets and the place the measured results land.

## Targets

| Metric | Target | Meaning |
|---|---|---|
| ISR entry → first instruction | < 2 µs | vector dispatch, no window spill |
| ISR execution (our handlers) | < 10 µs | contract for every handler |
| ISR → task wake (posted signal to task poll) | ≤ 50 µs p99 @ 240 MHz | the number that matters for control loops |
| Timer wake jitter | ≤ 100 µs | tickless deadline accuracy |
| Critical section length | ≤ 20 µs | bounds the worst-case interrupt latency |

## Method

1. Toggle a free GPIO at ISR entry and again at exit.
2. Toggle it a third time at the start of the woken task.
3. Capture with a logic analyzer or scope (≥ 10 MS/s), 1 000 events minimum.
4. Report p50 / p99 / max for: ISR width, ISR→task delay, and timer jitter.

Without a scope, the fallback is a free-running cycle counter sampled in the
ISR and in the task, at the cost of losing vector-entry time.

The harness lives in the example (`led` heartbeat is the placeholder) and will
move to a dedicated `bench-latency` image when the SoC interrupt layer is wired.

## Results

| Build | Board | ISR width | ISR→task p99 | Timer jitter | Notes |
|---|---|---|---|---|---|
| SDK hello-kernel, `irq-selftest` | ESP32-S3 rev0.2 DevKitC | — | 5–16 µs (ISR→main poll; not vector entry) | 100 µs one-shot, no misses | 2026-11-22, UART log |

Host tests cover the *policies* (IRQ table dispatch counts, scheduler deadlines,
overrun accounting) but cannot measure silicon latency. The first hardware run
fills this table and is a Phase 1 exit gate.

## What is already enforced in code

- `kernel/irq.rs` rejects duplicate claims and priorities outside 1–7, and counts
  dispatches so a storm is visible in diagnostics.
- `kernel/sched.rs` counts periodic overruns when a deadline is missed by more
  than one period.
- `kernel/supervisor.rs` reports a missed heartbeat once per miss and rearms the
  watchdog deadline.
