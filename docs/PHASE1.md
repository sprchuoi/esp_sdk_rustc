# Phase 1 status and execution order

Updated after the first real-hardware run (ESP32-S3 rev0.2, 16 MB flash, 8 MB PSRAM).

## Status matrix

| Item | State | Evidence | Remaining |
|---|---|---|---|
| **Console line editing** | done | `LineEditor`, 4 host tests, sim `--console` | — |
| **Console commands** | partial | `help`, `version`, `sched`, `status`, `mem`, `selftest`, `reset`, `rwdt-starve` registered; verified in sim `--console` | `tasks` and live `sched`/`log` need the shared monitor; no hardware RX |
| **UART RX** | **broken on hardware** | esp-println raw FIFO *and* the new `esp_hal::uart::Uart` backend both receive nothing; TX/flashing work | check CH343 host→board direction, UART0 pin mux, or `CONF0/CONF1`; commands are verified in sim meanwhile |
| **Scheduler integration** | partial | classes, round-robin, suspend/resume, aging, stats all host-tested | executor (8 KiB per class), `spawn`/`spawn_periodic`, `yield` |
| **Timers** | mostly done | deadline heap (32), tickless `hal::Timer` + TIMG one-shot, overrun accounting | hardware periodic wake integration (executor) |
| **Accounting** | partial | `begin_run`/`end_run`, wakes, max, overruns, `write_stats` | CCOUNT/1 kHz sampler; max latency; target rendering |
| **Interrupts** | partial | IRQ table/priorities/ISR contract host-tested; **TIMG0 ISR fires and returns on hardware** (5–16 µs observed in main) | ISR→task wake (currently main polling), bounded critical sections, true vector-entry latency |
| **Sync + bus** | partial | `Signal`, `MessageQueue`, `Mailbox`, `Pipe`, `Semaphore`, `EventBus` lossy/reliable | `Mutex`/`Channel`/`PubSub` on critical sections; ISR contracts enforced |
| **Logging** | partial | level filter, ring, `flush` for panic dumps | defmt sinks (USB-Serial-JTAG/UART), 2 KiB ring config, panic-path transport |
| **Boot/safety** | partial | RWDT fed and starvation reset proven in sim; `BootManager`, crash record encode/decode, 3-strike safe mode | PSRAM init, heap/stack placement, panic → RTC RAM → reset on target |
| **Host tests** | done | run order, deadlines, overruns, starvation, bus drop policy, crash round-trip (191 tests) | executor and CCOUNT tests once implemented |

## Critical path

1. **UART RX on hardware.** Without it the console cannot be exercised on the board, and every target command below is unverifiable. Preferred: compile `soc::Serial` RX with `esp_hal::uart::Uart` behind a `hardware` feature; the REG_UPDATE stall only exists in esp32sim. Keep the raw FIFO path for the emulator.
2. **Monitor state + commands.** Introduce one `Monitor` singleton (scheduler stats, diagnostics, log ring, reset reason) and six commands: `status`, `tasks`, `sched`, `mem`, `log`, `reset`, `selftest`. This forces the state plumbing the executor will reuse.
3. **Executor.** Per-class run loop, 8 KiB stack per class, `spawn`/`spawn_periodic`, `yield`, driven by `next_deadline()` + `Timer::arm`.
4. **CCOUNT sampler.** 1 kHz ISR samples the cycle counter; per-task deltas feed `TaskStats`; `sched` renders them.
5. **ISR→task wake + critical sections.** Handler posts to `Signal`/`Channel`; a `critical-section` implementation makes `Mutex`/`Channel`/`PubSub` sound.
6. **Logging sinks.** defmt over USB-Serial-JTAG/UART, 2 KiB ring, panic flush.
7. **Boot safety.** PSRAM init, heap/stack placement, panic writes `CrashRecord` to RTC RAM and resets; `BootManager` enters safe mode after three bad boots.
8. **Latency.** GPIO toggle in the ISR, scope or cycle-counter measurement; fill [latency.md](latency.md).

## Acceptance per step

- RX: `--send=$'help\nversion\n'` returns both outputs on the board.
- Commands: each command has a scripted sim assertion **and** a hardware log line.
- Executor: host test proves class ordering, `spawn_periodic` overruns and `yield`; hardware log shows tasks running.
- CCOUNT: `sched` shows non-zero per-task run time on hardware.
- ISR→task: handler wakes a task, not a polling loop; measured latency recorded.
- Logging: panic dump contains retained records before reset.
- Boot safety: three induced panics land in safe mode; fourth boot is healthy.
