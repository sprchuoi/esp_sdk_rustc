# Phase 1 status and execution order

Updated after the first real-hardware run (ESP32-S3 rev0.2, 16 MB flash, 8 MB PSRAM).

## Status matrix

| Item | State | Evidence | Remaining |
|---|---|---|---|
| **Console line editing** | done | `LineEditor`, 4 host tests, sim `--console` | — |
| **Console commands** | partial | `help`, `version`, `sched`, `status`, `mem`, `selftest`, `reset`, `rwdt-starve` registered; verified in sim `--console` | `tasks` and live `sched`/`log` need the shared monitor; no hardware RX |
| **UART RX** | **broken on hardware** | esp-println raw FIFO *and* `esp_hal::uart::Uart` (GPIO44 RX) both receive nothing; TX and flashing over the same CH343 work | loopback GPIO43→GPIO44, verify U0RXD mux in the GPIO matrix, inspect `CONF0/CONF1`; commands are verified in sim meanwhile |
| **Scheduler integration** | partial | classes, round-robin, suspend/resume, aging, stats all host-tested | executor (8 KiB per class), `spawn`/`spawn_periodic`, `yield` |
| **Timers** | mostly done | deadline heap (32), tickless `hal::Timer` + TIMG one-shot, overrun accounting | hardware periodic wake integration (executor) |
| **Accounting** | partial | `begin_run`/`end_run`, wakes, max, overruns, `write_stats` | CCOUNT/1 kHz sampler; max latency; target rendering |
| **Interrupts** | partial | IRQ table/priorities/ISR contract host-tested; **TIMG0 ISR fires and returns on hardware** (5–16 µs observed in main) | ISR→task wake (currently main polling), bounded critical sections, true vector-entry latency |
| **Sync + bus** | partial | `Signal`, `MessageQueue`, `Mailbox`, `Pipe`, `Semaphore`, `EventBus` lossy/reliable | `Mutex`/`Channel`/`PubSub` on critical sections; ISR contracts enforced |
| **Logging** | partial | level filter, ring, `flush` for panic dumps | defmt sinks (USB-Serial-JTAG/UART), 2 KiB ring config, panic-path transport |
| **Boot/safety** | partial | RWDT fed and starvation reset proven in sim; `BootManager`, crash record encode/decode, 3-strike safe mode | PSRAM init, heap/stack placement, panic → RTC RAM → reset on target |
| **Host tests** | done | run order, deadlines, overruns, starvation, bus drop policy, crash round-trip (191 tests) | executor and CCOUNT tests once implemented |

## Session evidence (2026-11-22)

- Flashed SDK image on ESP32-S3 rev0.2: boots, banner, console help, I2C scan,
  `timer: boot_us=167777`, scheduler and diagnostics all run.
- TIMG0 ISR (`irq-selftest`) fires and returns on hardware; ISR→main observation
  was 5–16 µs over five interrupts, no faults. The esp32sim exception storm is
  emulator-specific.
- `status/mem/reset/selftest` are registered and verified in esp32sim; no
  hardware RX means they cannot be invoked on the board yet.
- `--flash`, `--monitor`, `--send`, `--hw` and `target/serial.log` are the
  hardware workflow.

## Critical path

1. **UART RX root cause.** Without it the console cannot be exercised on the board, and every target command below is unverifiable. Preferred: compile `soc::Serial` RX with `esp_hal::uart::Uart` behind a `hardware` feature; the REG_UPDATE stall only exists in esp32sim. Keep the raw FIFO path for the emulator.
2. **Monitor state + remaining commands.** One `Monitor` singleton (scheduler
   stats, diagnostics, log ring, reset reason) feeding `tasks`, live `sched`,
   `log` (and the existing `status/mem/selftest`). This is the state plumbing the
   executor reuses; it needs a `critical-section` implementation, which is also
   step 5's prerequisite.
3. **Executor.** Per-class run loop, 8 KiB stack per class, `spawn`/`spawn_periodic`, `yield`, driven by `next_deadline()` + `Timer::arm`.
4. **CCOUNT sampler.** 1 kHz ISR samples the cycle counter; per-task deltas feed `TaskStats`; `sched` renders them.
5. **ISR→task wake + critical sections.** Handler posts to `Signal`/`Channel`; a `critical-section` implementation makes `Mutex`/`Channel`/`PubSub` sound.
6. **Logging sinks.** defmt over USB-Serial-JTAG/UART, 2 KiB ring, panic flush.
7. **Boot safety.** PSRAM init, heap/stack placement, panic writes `CrashRecord` to RTC RAM and resets; `BootManager` enters safe mode after three bad boots.
8. **Latency.** GPIO toggle in the ISR, scope or cycle-counter measurement; fill [latency.md](latency.md).

## Immediate next actions

1. Loopback test: build a variant that connects U0TXD to U0RXD in the GPIO
   matrix, send one byte, read it back. This separates "pin mux wrong" from
   "CH343 RX net not wired".
2. If loopback passes, fix the external RX path/board wiring claim; if it fails,
   program `CONF0/CONF1` explicitly and re-test with a scope on GPIO44.
3. In parallel (sim-verifiable): add `Monitor` + `tasks`/`log`/live `sched`
   commands, using the same `critical-section` backend the ISR/Mutex work needs.

## Acceptance per step

- RX: `--send=$'help\nversion\n'` returns both outputs on the board.
- Commands: each command has a scripted sim assertion **and** a hardware log line.
- Executor: host test proves class ordering, `spawn_periodic` overruns and `yield`; hardware log shows tasks running.
- CCOUNT: `sched` shows non-zero per-task run time on hardware.
- ISR→task: handler wakes a task, not a polling loop; measured latency recorded.
- Logging: panic dump contains retained records before reset.
- Boot safety: three induced panics land in safe mode; fourth boot is healthy.
