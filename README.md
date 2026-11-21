# esp_sdk_rustc

A Rust SDK with its own tiny RTOS for ESP32 smart-home nodes — configuration-driven and portable
the way a Linux kernel is: Kconfig for features, device tree for hardware, drivers bound by
`compatible` string.

**Status: planning.** No firmware yet — the design and roadmap are in
[docs/PLAN.md](docs/PLAN.md), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md),
[docs/CONFIGURATION.md](docs/CONFIGURATION.md) and [docs/DRIVERS.md](docs/DRIVERS.md).

## The idea

An ESP32 smart-home node should be a manifest, not a codebase:

```rust
// apps/sensor-node/src/main.rs — the whole app
const MANIFEST: NodeManifest = NodeManifest {
    id: "bme280-01",
    entities: &[
        Entity::sensor(SensorKind::Temperature, "temperature", "Temperature"),
        Entity::sensor(SensorKind::Humidity, "humidity", "Humidity"),
        Entity::switch("relay1", "Lamp"),
    ],
    ..Default::default()
};

app!(MANIFEST, board = boards::Esp32S3DevKitC1);
```

Everything else — scheduler, interrupts, logging, crypto, DMA, EEPROM/flash, WiFi, MQTT, Home
Assistant discovery, OTA, sleep, watchdog, config storage — is the SDK's job.

## Portability model

| Linux | This SDK |
|---|---|
| `Kconfig` → `.config` | same, vendored kconfiglib → generated `sdkconfig.rs` |
| `arch/`, `soc/` | `arch/{xtensa,riscv}`, `soc/{esp32s3,esp32c6}` |
| `.dts` → `.dtb` | `board.dts` → `dtc` → parse at build time → `BoardIR` |
| `of_match_table` | `DriverFactory::COMPATIBLE` + probe registry |
| per-board defconfig | `boards/<board>/defconfig` |
| `allyesconfig` in CI | same, so disabled drivers cannot rot |

A new **board** is a `.dts` + defconfig. A new **driver** is one module + one Kconfig symbol. A new
**SoC** is one `soc/` crate. The kernel does not change for any of them.

## What's in the box

| Capability | What you get |
|---|---|
| **Configuration** | Kconfig tree, `menuconfig`, `savedefconfig`, generated Rust consts + cfg flags, `allyesconfig` CI |
| **Board model** | DTS with buses/devices/partitions, build-time validation (unknown compatible, duplicate address, overlapping partition = build error) |
| **Scheduler** | priority classes (`Ctrl/App/Net/Idle`), tickless timers, CPU accounting, overrun detection |
| **Interrupts** | static IRQ table, ISR→task wake path, bounded critical sections, measured latency |
| **Logging** | `defmt`, runtime level filter, RAM ring, 64 KiB flash ring, crash dump surviving reset |
| **Crypto** | SHA-256, HMAC, AES-GCM, TRNG, Ed25519 — HW accelerators + RustCrypto fallback, KAT-tested |
| **EEPROM** | AT24 (I2C), 25LC (SPI), page-boundary writes, write-cycle timing, `EepromKv` for boards without flash |
| **FLASH / MTD** | range-checked partitions, KV, forensic log, OTA slots, wear counters, `embedded-storage` adapter |
| **DMA** | move-owned buffers, static pools, async SPI/UART/ADC, cache-safe PSRAM path |
| **NET** | `NetDevice` trait, WiFi + W5500 Ethernet, smoltcp/DHCP, provisioning portal, MQTT, SNTP |
| **Sleep** | light sleep, WiFi modem sleep, deep sleep profile, wake sources, measured current |
| **OTA** | dual slot, SHA-256 + Ed25519, rollback on failed boot, safe mode |

## Shape of the stack

```
apps/            sensor-node · actuator-node
subsys/          provisioning · mqtt + HA discovery · ota · telemetry · storage
drivers/         bus · eeprom · mtd · net · gpio · sensor
kernel/          scheduler · interrupts · time · sync · driver core · kv · log · power
arch/ soc/       xtensa + esp32s3 · riscv + esp32c6        ← portability boundary
silicon          esp-hal 1.2 + esp-radio + embassy          (rented, not written)
```

Rust `no_std`, async-first, static allocation, dual-slot OTA with rollback. First target
**ESP32-S3** (DevKitC-1 N16R8), then **ESP32-C6** as the proof that the abstraction holds.

## Roadmap

| Milestone | Content | Target |
|---|---|---|
| M0 | Toolchain, skeleton, CI, blink + log | week 1 |
| M1 | TinyRTOS: scheduler, interrupts, logging, watchdog, console | week 4 |
| M2 | Kconfig + DTS + codegen + driver core + buses | week 7 |
| M3 | EEPROM, MTD/flash, KV, DMA, crypto KATs on hardware | week 11 |
| M4 | WiFi netdev, provisioning, MQTT, 72 h soak | week 14 |
| M5 | HA discovery, actuators, signed OTA with rollback | week 17 |
| M6 | Sleep/power, C6 port, 7-day soak → v1.0 | week 20 |

## Non-goals

Matter stack, cloud backend, voice assistant, a new LLVM backend, a new WiFi PHY.

## Documents

- [Plan](docs/PLAN.md) — phases, decisions (ADR-001…016), milestones, risks
- [Architecture](docs/ARCHITECTURE.md) — scheduler, interrupts, DMA, crypto, sleep, memory map
- [Configuration](docs/CONFIGURATION.md) — Kconfig, board DTS, codegen, porting recipes, CI matrix
- [Drivers](docs/DRIVERS.md) — driver model, bus core, EEPROM, MTD/flash, NET/WiFi

## License

MIT OR Apache-2.0 (to be added in Phase 0).
