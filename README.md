# esp_sdk_rustc

[![CI](https://github.com/sprchuoi/esp_sdk_rustc/actions/workflows/ci.yml/badge.svg)](https://github.com/sprchuoi/esp_sdk_rustc/actions/workflows/ci.yml)

A Rust **SDK** with its own tiny RTOS for ESP32 smart-home nodes — configuration-driven and portable
the way a Linux kernel is: Kconfig for features, device tree for hardware, drivers bound by
`compatible` string.

Customers depend on it to build their own products. Everything under `apps/` here is an example; the
first real consumer is the migration of [`smart_home_idf`](https://github.com/sprchuoi/smart_home_idf)
from ESP-IDF/C++ onto this SDK.

**Status: Phase 2 seed.** `hal/` has the trait set plus host mocks; `kernel/` has the scheduler,
time, `Signal`, log ring, service manager, SHA-256/HMAC, power policy and diagnostics (metrics,
memory low-water, faults, crash dump); `drivers/` has the binding model and an AT24 EEPROM driver;
`boot/` has partition validation, otadata selection and app-descriptor parsing —
**120 host tests**, clean `no_std` build for a bare-metal target. The rest is documentation and the
phase plan:
[PLAN](docs/PLAN.md) · [ARCHITECTURE](docs/ARCHITECTURE.md) · [CONFIGURATION](docs/CONFIGURATION.md) ·
[DRIVERS](docs/DRIVERS.md) · [BOOT](docs/BOOT.md) · [CI](docs/CI.md) · [SDK contract](docs/SDK.md) ·
[MIGRATION](docs/MIGRATION.md).

## Using the SDK

```toml
[dependencies]
esp-sdk = { version = "0.1", features = ["esp32s3", "wifi", "mqtt", "smarthome", "flash"] }

[build-dependencies]
esp-sdk-build = "0.1"
```

```rust
// build.rs — the board is data, not code
fn main() { esp_sdk_build::board("board.dts").unwrap(); }
```

```rust
// src/main.rs — the whole app
#![no_std]
#![no_main]

use esp_sdk::prelude::*;

const MANIFEST: NodeManifest = NodeManifest {
    id: "living-room",
    entities: &[
        Entity::sensor(SensorKind::Temperature, "temperature", "Temperature"),
        Entity::switch("relay1", "Lamp"),
    ],
    ..Default::default()
};

app!(MANIFEST);
```

```bash
cargo esp-sdk build     # target + features + board codegen, one command
cargo esp-sdk flash
```

`cargo esp-sdk` is convenience, not a lock-in: a plain
`cargo build --target xtensa-esp32s3-none-elf --features esp32s3,wifi,mqtt` works too.

## Portability model

| Linux | This SDK |
|---|---|
| `Kconfig` → `.config` | same, vendored kconfiglib → generated `sdkconfig.rs` (authoring/CI) |
| `arch/`, `soc/` | `arch/{xtensa,riscv}`, `soc/{esp32s3,esp32c6}` |
| `.dts` → `.dtb` | customer `board.dts` → `dtc` → build-time parse → board tables |
| `of_match_table` | `DriverFactory::COMPATIBLE` + generated probe registry |
| per-board defconfig | in-tree `boards/<board>/defconfig`; features downstream |
| `allyesconfig` CI | same, so disabled drivers cannot rot |

A new **board** is a `.dts` + defconfig. A new **driver** is one module + one Kconfig symbol. A new
**SoC** is one `soc/` crate. The kernel does not change for any of them.

## What's in the box

| Capability | What you get |
|---|---|
| **Published SDK** | `esp-sdk` facade, implementation crates, `esp-sdk-build`, `cargo-esp-sdk` CLI, examples |
| **Configuration** | Kconfig, `menuconfig`, `savedefconfig`, generated Rust, `allyesconfig` CI |
| **Board model** | DTS with buses/devices/partitions; unknown compatible, duplicate address, overlapping partition = build error |
| **Scheduler** | priority classes (`Ctrl/App/Net/Idle`), tickless timers, CPU accounting, overrun detection |
| **Interrupts** | static IRQ table, ISR→task wake path, bounded critical sections, measured latency |
| **Logging** | `defmt`, runtime level filter, RAM ring, 64 KiB flash ring, crash dump surviving reset |
| **Crypto** | SHA-256, HMAC, AES-GCM, TRNG, Ed25519 — HW accelerators + RustCrypto fallback, KAT-tested |
| **EEPROM** | AT24 (I2C), 25LC (SPI), page-boundary writes, write-cycle timing, `EepromKv` |
| **FLASH / MTD** | range-checked partitions, KV, forensic log, OTA slots, wear counters |
| **DMA** | move-owned buffers, static pools, async SPI/UART/ADC, cache-safe PSRAM path |
| **NET** | `NetDevice` trait, WiFi + W5500 Ethernet, smoltcp/DHCP, provisioning portal, MQTT, SNTP |
| **Sleep** | light sleep, WiFi modem sleep, deep sleep profile, wake sources, measured current |
| **OTA** | dual slot, SHA-256 + Ed25519, rollback on failed boot, safe mode |

## Shape of the stack

```
apps/            sensor-node · actuator-node          (examples / reference products)
boot/            partitions · otadata · selection · app descriptor
subsys/          provisioning · mqtt + HA discovery · ota · storage · crypto
drivers/         bus · eeprom · mtd · net · gpio · sensor
kernel/          scheduler · interrupts · time · sync · driver core · kv · log · power
hal/ arch/ soc/  traits · xtensa + esp32s3 · riscv + esp32c6   ← portability boundary
silicon          esp-hal 1.2 + esp-radio + embassy              (rented, not written)
```

Rust `no_std`, async-first, static allocation. First target **ESP32-S3** (DevKitC-1 N16R8), then
**ESP32-C6** as the proof that the abstraction holds.

## Roadmap

| Milestone | Content | Target |
|---|---|---|
| M0 | Toolchain, skeleton, CI, blink + log | week 1 |
| M1 | TinyRTOS: scheduler, interrupts, logging, watchdog, console | week 4 |
| M2 | Kconfig + DTS + codegen + driver core + buses | week 7 |
| M3 | EEPROM, MTD/flash, KV, DMA, crypto on hardware | week 11 |
| M4 | WiFi netdev, provisioning, MQTT, 72 h soak | week 14 |
| M5 | HA discovery, actuators, signed OTA with rollback | week 17 |
| M6 | **SDK 0.1 published** — external consumer builds | week 19 |
| M7 | **`smart_home_idf` migrated**, contract parity on hardware | week 22 |
| M8 | Sleep/power, C6 port, 7-day soak → **v1.0** | week 25 |

## Migration

`smart_home_idf` runs on the SDK when the broker contract is unchanged: topics, payloads, LWT,
console commands, OTA slot layout. The C++ repo's docs and code disagree on the topic schema, so the
contract is captured from the running firmware into golden fixtures and diffed against a shadow node
before any cutover. Plan: [docs/MIGRATION.md](docs/MIGRATION.md).

## Development

```bash
cargo test --workspace --all-features          # 120 host tests (HAL, kernel, drivers, boot)
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo check --workspace --all-features --target x86_64-unknown-none   # proves no_std
```

CI runs exactly these on every push, plus MSRV 1.95; S3/C6 device builds, the config matrix and
hardware-in-the-loop activate automatically as those pieces land ([docs/CI.md](docs/CI.md)).

## Non-goals

Matter stack, cloud backend, voice assistant, a new LLVM backend, a new WiFi PHY.

## Documents

- [Plan](docs/PLAN.md) — phases, ADR-001…019, milestones, risks
- [Architecture](docs/ARCHITECTURE.md) — scheduler, interrupts, DMA, crypto, sleep, memory map
- [Configuration](docs/CONFIGURATION.md) — Kconfig, board DTS, codegen, porting, CI matrix
- [Drivers](docs/DRIVERS.md) — driver model, buses, EEPROM, MTD/flash, NET
- [Boot](docs/BOOT.md) — partition table, otadata, rollback flow, app descriptor
- [CI](docs/CI.md) — pipeline, test layers, HIL, release gates
- [SDK contract](docs/SDK.md) — crates, features, versioning, release policy
- [Migration](docs/MIGRATION.md) — `smart_home_idf` parity and cutover

## License

MIT OR Apache-2.0 (to be added in Phase 0).
