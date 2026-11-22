# Kernel pathway and IPC

**Status:** draft v0.1 · 2026-09-21
How the kernel boots, how the pieces connect, and what each layer may call — modelled on the Linux
kernel's initcall/subsystem split, reduced to a 512 KiB MCU.

---

## 1. The boot pathway

```
reset
  │
  ├─ arch entry            arch/xtensa · arch/riscv      (asm, IRQ vectors, linker)
  │
  ├─ kernel init           kernel/                       early + core initcalls
  │    clock · heap · console · scheduler · timers · IRQ dispatch
  │
  ├─ initcalls             kernel/subsys                 by level, stable order
  │    early → core → postcore → arch → subsys → fs → device → late
  │
  ├─ board probes          drivers/model                 compatible matching, deferred probe
  │
  ├─ subsystem init        net · storage · crypto        registers devices/services
  │
  ├─ services start        kernel/service                dependency order, restart policy
  │
  └─ scheduler running     kernel/sched                   never returns
```

This mirrors Linux's `do_initcalls()`: a component does not call another component's `init`
directly; it registers an initcall at a level and the boot path orders them. Registration is static
(`InitTable`), so there is no linker section, no function pointer table and no allocation.

Why reproduce the level system at all: it makes the *dependencies* explicit. A driver that needs the
flash driver cannot run at `device` level and expect `fs` to be up; the level says so, and the order
is computed rather than implied by the order of `main()` statements.

## 2. Linux mapping

| Linux | TinyRTOS | Notes |
|---|---|---|
| initcall levels (`early`…`late`) | `kernel::subsys::InitLevel` | same names and order |
| `subsys_initcall`, `device_initcall` | `InitTable::register(name, level)` | static table, no macros yet |
| `struct bus_type` + `driver_register` | `drivers::model::{DeviceNode, DriverDesc}` | compatible string matching |
| `netdev` | `esp-sdk-net` service + `hal::NetDevice` (Phase 4) | link/lease state machine now |
| pipes, message queues, semaphores | `kernel::ipc` | non-blocking primitives only |
| systemd units | `kernel::service` | dependency order + restart policy |
| `/proc/meminfo`, `/proc/interrupts` | `kernel::metrics` dump | `key=value` lines, no VFS |
| kmsg / printk | `kernel::log` + defmt | ring retained for crash dump |

**What we deliberately do not copy:** VFS, character devices, sockets, modules/hotplug, `ioctl`,
`/sys`. Those exist in Linux to serve user space; a single-address-space firmware gets their value
from typed traits and static tables instead.

## 3. IPC

`kernel/src/ipc.rs`:

| Primitive | Shape | Use |
|---|---|---|
| `MessageQueue<T, N>` | bounded FIFO, `push`/`pop`, `push_overwrite`, high-water mark | ISR → task events, command batches, telemetry backlog |
| `Mailbox<T>` | one slot, keep newest, count overwrites | "latest reading", "latest command" |
| `Pipe<N>` | byte ring, partial read/write | console input, log byte streams |
| `Semaphore` | atomic counter, `try_wait`/`signal` | resource tokens (buffer slots, DMA channels) |

Design constraints:

- **No blocking calls in the primitives.** A blocking `wait` needs a waker and an arch critical
  section; those arrive with the executor integration. `Signal` (already present) is the wakeup
  half, and the executor composes it with these queues.
- **No interior mutability tricks.** The primitives take `&mut self`; sharing is the executor's job
  (single owner per queue) or the arch's (critical section). This keeps the kernel
  `#![forbid(unsafe_code)]`.
- **Backpressure is visible.** `push` returns the item when full; `push_overwrite` returns the
  dropped item; both have high-water marks so `Diagnostics` can show queue pressure.

## 4. Subsystems and initcalls

`kernel/src/subsys.rs`:

```rust
let mut init = InitTable::<16>::new();
init.register("clock", InitLevel::Early)?;
init.register("sched", InitLevel::Core)?;
init.register("net",   InitLevel::Subsys)?;
init.register("mqtt",  InitLevel::Late)?;

let mut order = [0u8; 16];
let order = init.order(&mut order)?;     // by level, then registration order
```

`SubsystemTable` adds a class (`Core`, `Net`, `Storage`, `Input`, `Crypto`, `Diagnostics`) used by
diagnostics and by the board generator to group devices. A subsystem is a *declaration*; the service
manager owns runtime lifecycle, and the driver model owns device binding. Three different questions:

- **initcalls** — "in what order does boot run?"
- **drivers** — "which driver owns this device tree node?"
- **services** — "is this component running, and should it restart?"

## 5. Network service

`net/` (`esp-sdk-net`) owns the service view of connectivity, not the packet path:

```
Down ──link_up──► RequestingLease ──lease_acquired──► Ready { address, gateway, expires }
  ▲                    │                                   │
  └────link_down───────┴────lease_failed──► Degraded { retry_at } ──tick──► RequestingLease
```

- `tick(now)` expires the lease and schedules retries; `next_deadline()` tells the timer arm where to
  point.
- `Ipv4Addr` parses/formats without allocation.
- The device side is `hal::NetDevice` (`name`, `mac`, `mtu`, `up`/`down`, `link`, `stats`,
  `transmit`/`receive`), already defined; the smoltcp/`esp-radio` implementation plugs in behind it
  in Phase 4. The state machine above is already tested for link flaps and lease expiry.

## 6. Directory layout

```
kernel/   sched · irq · time · sync · ipc · subsys · service · crypto · power · metrics · log
hal/      traits only (bus, GPIO, flash, netdev) + host mocks
drivers/  model (binding) · eeprom · mtd · net · sensor
net/      link/lease state machine, address types
boot/     partitions · otadata · selection · app descriptor
soc/      esp32s3 · esp32c6 (Phase 2)
subsys/   storage · crypto backends · provisioning · ota (Phase 3+)
```

## 7. Simulation with esp32sim

[`mikroverk/esp32sim`](https://github.com/mikroverk/esp32sim) is vendored as the
`third_party/esp32sim` submodule. It boots the real mask ROM, the real second-stage bootloader and an
unmodified application image, which is exactly the artifact our `boot/` code reasons about.

Working flow:

```bash
git submodule update --init --depth 1
(cd third_party/esp32sim && tools/fetch-rom-elfs.sh web/wasm/fw)
./scripts/build-sim-image.sh hello-kernel     # example ELF -> app image + partition table + bootloader
./scripts/sim-smoke.sh                        # boots it and asserts the kernel ran
```

`sim-smoke.sh` checks that the banner appeared, `[ctrl] tick` fired, the diagnostics dump was
produced and `mqtt.packets` shows IPC traffic. Current output (8 emulated seconds):

```
 esp-sdk hello-kernel on ESP32-S3
[ctrl] tick t=0 ms
[net ] publish sensor=0 t=0 ms
...
uptime_ms=5000 boot=1 health=ok
mem heap cap=65536 used=12288 free=53248 low=53248
metric mqtt.packets=9
metric sensor.reads=10
```

Honest limits:

- The mask ROM is not in the repository; `tools/fetch-rom-elfs.sh` downloads it (Apache-2.0).
- The bootloader is a committed ESP-IDF v5.5.1 S3 build (`boards/esp32s3-devkitc-1/bootloader.bin`)
  so the image is reproducible without an IDF install. Phase 0 replaces it with a reproducible
  rollback-enabled build.
- The emulator models the S3 well but is not a substitute for the real board: timing, power and RF
  are simulated. It is a fast regression net for the boot path, not the acceptance gate.
- The CI job stays gated behind the `SIMULATION` variable (it downloads the toolchain and builds the
  emulator, ~10 min); a green pipeline must not imply emulator coverage that did not run.

## 8. What the pathway guarantees

1. A component never calls another component's init directly; levels and dependencies decide order.
2. A driver never reads board constants; the device tree provides them.
3. A service never starts before its dependencies; the manager enforces and reports it.
4. A network consumer only runs when a lease exists; the state machine gates it.
5. Every failure is recorded (fault table, service state, crash record) rather than printed once.
