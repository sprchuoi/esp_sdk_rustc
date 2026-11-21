# esp_sdk_rustc

A Rust SDK with its own tiny RTOS for ESP32 smart-home nodes.

**Status: planning.** No firmware yet — the design and roadmap are in
[docs/PLAN.md](docs/PLAN.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## The idea

An ESP32 smart-home node should be a manifest, not a codebase. This SDK aims for:

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

Everything else — scheduler, interrupts, logging, crypto, DMA, WiFi, MQTT, Home Assistant
discovery, OTA, sleep management, watchdog, config storage — is the SDK's job.

## What's in the box

| Capability | What you get |
|---|---|
| **Scheduler** | priority classes (`Ctrl/App/Net/Idle`), tickless timers, round-robin within class, CPU accounting, overrun detection, starvation guard |
| **Interrupts** | static IRQ table, ISR→task wake path, bounded critical sections, measured latency |
| **Logging** | `defmt`, runtime level filter, RAM ring, 64 KiB flash ring, crash dump that survives reset |
| **Crypto** | SHA-256, HMAC, AES-GCM, TRNG, Ed25519 — hardware accelerators + RustCrypto fallback, KAT-tested |
| **DMA** | move-owned buffers, static pools, async SPI/UART/ADC transfers, cache-safe PSRAM path |
| **Sleep** | light sleep, WiFi modem sleep, deep sleep profile, wake sources, measured current |
| **Storage** | atomic log-structured KV, sealed secrets, forensic log partition |
| **Network** | WiFi provisioning (serial + portal), MQTT with LWT/QoS1, SNTP, HA discovery |
| **OTA** | dual slot, SHA-256 + Ed25519, rollback on failed boot, safe mode |

## Shape of the stack

```
apps/            sensor-node · actuator-node
services/        provisioning · mqtt + HA discovery · ota · telemetry
drivers/         gpio · i2c/spi/uart · pwm · adc · dma
kernel (TinyRTOS) scheduler · interrupts · time · sync · bus · kv · crypto · log · power
arch/            esp32s3 · esp32c6 · host mock     ← the portability boundary
silicon          esp-hal 1.2 + esp-radio + embassy  (rented, not written)
```

Rust `no_std`, async-first, static allocation, dual-slot OTA with rollback. First target
**ESP32-S3** (DevKitC-1 N16R8), then **ESP32-C6** as the proof that the abstraction holds.

## Roadmap

| Milestone | Content | Target |
|---|---|---|
| M0 | Toolchain, skeleton, CI, blink + log | week 1 |
| M1 | TinyRTOS: scheduler, interrupts, logging, watchdog, console | week 4 |
| M2 | Drivers, DMA, KV, crypto KATs on hardware | week 7 |
| M3 | WiFi + MQTT + provisioning, 72 h soak | week 10 |
| M4 | HA discovery, actuators, signed OTA with rollback | week 13 |
| M5 | Sleep/power profiles, C6 port, 7-day soak → v1.0 | week 16 |

## Non-goals

Matter stack, cloud backend, voice assistant, a new LLVM backend, a new WiFi PHY.

## Documents

- [Plan](docs/PLAN.md) — phases, decisions (ADR-001…012), milestones, risks
- [Architecture](docs/ARCHITECTURE.md) — scheduler, interrupts, DMA, crypto, sleep, memory map, device model

## License

MIT OR Apache-2.0 (to be added in Phase 0).
