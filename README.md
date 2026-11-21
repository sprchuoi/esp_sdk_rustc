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

Everything else — WiFi provisioning, MQTT, Home Assistant discovery, OTA, watchdog, config
storage — is the SDK's job.

## Shape of the stack

```
apps/            sensor-node · actuator-node
services/        provisioning · mqtt + HA discovery · ota · telemetry
drivers/         gpio · i2c sensors · pwm · adc · onewire
kernel (TinyRTOS) tasks · time · sync · event bus · kv store · power · supervisor
arch/            esp32s3 · esp32c6 · host mock     ← the portability boundary
silicon          esp-hal 1.2 + esp-radio + embassy  (rented, not written)
```

Rust `no_std`, async-first, static allocation, dual-slot OTA with rollback. First target
**ESP32-S3** (DevKitC-1 N16R8), then **ESP32-C6** as the proof that the abstraction holds.

## Roadmap

| Milestone | Content | Target |
|---|---|---|
| M0 | Toolchain, skeleton, CI, blink + log | week 1 |
| M1 | Kernel v0: tasks, time, sync, console, watchdog | week 3 |
| M2 | Sensor node on hardware, persisted config | week 5 |
| M3 | WiFi + MQTT + provisioning, 72 h soak | week 8 |
| M4 | HA discovery, actuators, OTA with rollback | week 11 |
| M5 | C6 port, power management, 7-day soak → v1.0 | week 14 |

## Non-goals

Matter stack, cloud backend, voice assistant, a new LLVM backend, a new WiFi PHY.

## Documents

- [Plan](docs/PLAN.md) — phases, decisions (ADR-001…008), milestones, risks
- [Architecture](docs/ARCHITECTURE.md) — boot flow, kernel, traits, memory map, device model

## License

MIT OR Apache-2.0 (to be added in Phase 0).
