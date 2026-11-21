# SDK Contract — what customers depend on

**Status:** draft v0.1 · 2026-09-21
This repository is the **SDK**, not a product. Apps under `apps/` are examples and reference
implementations. A customer depends on one crate, describes their board in one file, and ships.

---

## 1. What a customer sees

```toml
# Cargo.toml
[dependencies]
esp-sdk = { version = "0.3", features = ["esp32s3", "wifi", "mqtt", "smarthome", "flash", "eeprom"] }

[build-dependencies]
esp-sdk-build = "0.3"
```

```rust
// build.rs
fn main() {
    esp_sdk_build::board("board.dts").unwrap();   // validates + generates board tables
}
```

```rust
// src/main.rs
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

app!(MANIFEST);   // generates main, spawns services, runs the scheduler
```

```bash
cargo esp-sdk build          # target, linker, features, codegen — one command
cargo esp-sdk flash
```

`cargo build --target xtensa-esp32s3-none-elf --features esp32s3,wifi,mqtt` works too; the CLI is
convenience, not a lock-in.

---

## 2. Crate graph

| Crate | Role | Semver from 0.1 |
|---|---|---|
| **`esp-sdk`** | facade: re-exports the public API, selects features | stable-ish, additive |
| `esp-sdk-hal` | traits only (`Serial`, `I2cBus`, `SpiBus`, `Eeprom`, `Mtd`, `NetDevice`, …) | unstable |
| `esp-sdk-kernel` | scheduler, IRQ, time, sync, driver core, KV, log, power | unstable |
| `esp-sdk-drivers` | bus + EEPROM + MTD + GPIO/PWM/ADC + sensors | unstable |
| `esp-sdk-net` | netdev core, WiFi, DHCP, MQTT, SNTP, provisioning | unstable |
| `esp-sdk-smarthome` | manifest, capabilities, HA discovery, node state machine | unstable |
| `esp-sdk-boot` | partitions, `otadata`, image verification | unstable |
| `esp-sdk-crypto` | hash/HMAC/AEAD/RNG/signature | unstable |
| `esp-sdk-esp32s3`, `esp-sdk-esp32c6` | SoC implementations | unstable |
| `esp-sdk-build` | build-dependency: DTS → board tables, config validation | stable |
| `cargo-esp-sdk` | CLI: new, board, build, flash, monitor, menuconfig | stable |

Only `esp-sdk` appears in a customer's dependency list. The rest are implementation crates that the
facade re-exports; they are published so the graph resolves and so advanced users can pin them.

**Rule:** a published crate never reads a global `.config`, environment-specific file or absolute
path at build time. Reproducibility from `Cargo.lock` + feature list is non-negotiable for crates.io
consumers (and for docs.rs).

---

## 3. Features: the compile-time contract

Cargo features are the *only* mechanism a published crate can reliably receive configuration, so the
SDK treats them as its public contract and keeps the list small and meaningful:

| Group | Features |
|---|---|
| SoC (exactly one) | `esp32s3`, `esp32c6` |
| Link | `wifi`, `ethernet-w5500` |
| Storage | `flash`, `eeprom`, `kv` |
| Protocol | `mqtt`, `smarthome`, `ota`, `sntp` |
| Crypto | `crypto-hw`, `crypto-ed25519` |
| Profiles | `node-sensor`, `node-actuator` (curated bundles of the above) |
| Development | `defmt`, `log`, `unstable` |

Rules:

- **Additive only.** Enabling a feature never removes API, never changes a default silently.
- **Mutually exclusive groups are compile errors**, not linker surprises: `compile_error!` for two
  SoCs, `ethernet-w5500` without `wifi` is allowed but `smarthome` without `mqtt` is not.
- **`unstable`** gates anything not yet covered by semver; it is documented in rustdoc.
- Profiles exist because 12 individual features are error-prone for the common cases; they expand to
  a documented bundle and can be combined with individual features.

Validation: `esp-sdk-build` checks that every `compatible` in `board.dts` has an enabled driver and
emits a precise error (`node /soc/i2c@60004000/eeprom@50: compatible "atmel,at24c256" needs feature
"eeprom"`). `cargo esp-sdk board` runs the same check without building.

---

## 4. Board description is data

The board file is the customer's only hardware description:

```dts
/dts-v1/;
/ {
    compatible = "acme,smart-plug-v2";
    model = "ACME Smart Plug v2";

    i2c0: i2c@60004000 {
        compatible = "espressif,esp32s3-i2c";
        clock-frequency = <400000>;
        eeprom@50 { compatible = "atmel,at24c256"; reg = <0x50>; size = <32768>; pagesize = <64>; };
    };

    relay0: relay { compatible = "gpio-relay"; gpios = <&gpio0 4 0>; };
    button0: button { compatible = "gpio-keys"; gpios = <&gpio0 0 1>; label = "pair"; };
};
```

`esp-sdk-build` compiles it with `dtc`, validates it (unknown compatible, duplicate address,
overlapping partition, missing bus) and generates `board.rs`. Everything else — pins, addresses,
partitions — never appears in Rust code.

Two ways to get the same result:

| Path | Mechanism | For |
|---|---|---|
| Plain Cargo | features in `Cargo.toml` + `build.rs` calling `esp_sdk_build::board()` | customers, docs.rs, minimal setups |
| Kconfig front-end | `cargo esp-sdk menuconfig` edits the feature set and writes `.config` + `features` for the CLI | teams that want the Linux-style menu, in-tree CI |

Kconfig remains the **authoring** interface inside this repo (`allyesconfig`, defconfig matrix); the
compiler boundary is always the feature list. That is the one place where distribution reality beats
the Linux analogy, and it is recorded as ADR-017.

---

## 5. Versioning and release policy

- **Lockstep versioning** across all crates while < 1.0; one tag `v0.x.y`, one changelog.
- **Publish order** (crates.io): `hal → kernel → crypto → boot → drivers → net → smarthome →
  soc-* → build → esp-sdk → cargo-esp-sdk`. `xtask publish --dry-run` walks the order and fails on
  a missing dependency version.
- **Semver enforcement:** `cargo semver-checks` on the public surface of `esp-sdk` and
  `esp-sdk-build`; `cargo public-api` snapshot diff reviewed in the release PR.
- **MSRV:** Rust 1.95 (matches `esp-hal` 1.2); tested in CI on the pinned toolchain only.
- **Deprecation:** `#[deprecated(since = "...")]` + a changelog entry + removal no earlier than the
  next minor.
- **API shape:** `#[non_exhaustive]` on public enums, no `unsafe` in public signatures, no
  `Box<dyn Trait>` in hot paths, error types implement `core::error::Error` where useful.
- **docs.rs:** every crate builds with `--cfg docsrs`, feature matrix documented; the facade has
  runnable host examples.
- **Supply chain:** `cargo-deny` (licenses, advisories, duplicate crates) in CI; `Cargo.lock`
  committed for the SDK repo, not required for consumers.

---

## 6. Support tiers

| Tier | Meaning |
|---|---|
| **1 — ESP32-S3** | built, flashed and tested on every release; latency/power numbers published |
| **2 — ESP32-C6** | built and bench-tested every release; no power claims |
| **3 — other ESP32** | community; compiles or it does not, no release gate |

`smart_home_idf` migration runs on Tier 1 hardware, so it is the reference platform.

---

## 7. Examples in this repo

| Example | Shows |
|---|---|
| `examples/blinky` | bare kernel + GPIO, no net |
| `examples/eeprom-config` | EEPROM + KV + sealed secrets |
| `examples/sensor-node` | BME280/BH1750 + MQTT + HA discovery |
| `examples/actuator-node` | relays, buttons, PWM dimmer |
| `examples/ota` | signed OTA + rollback |
| `examples/wifi-portal` | SoftAP provisioning (`POST /wifisave`) |
| `examples/ethernet-node` | W5500 netdev, same services |

CI builds every example for every Tier-1 defconfig; examples are also the documentation that cannot
rot. `apps/` holds the two reference products used by the migration, `examples/` the small ones.

---

## 8. What the SDK promises

- No `unsafe` outside `soc/` and `arch/` (enforced by lint + review).
- No panics on any driver error path; `Result` with typed errors.
- No allocation in kernel/drivers; `alloc` only in opt-in layers (TLS, large buffers).
- Every blocking call has a documented timeout; nothing spins forever.
- Every public item is documented; `#![deny(missing_docs)]` on the facade.
- Deterministic builds: pinned toolchain, `Cargo.lock` for the repo, version stamps in the app
  descriptor.

---

## 9. Open questions

1. Publish to crates.io under `esp-sdk`/`esp-sdk-*` (all free today), or distribute as a git
   dependency until 0.1 is stable?
2. Keep lockstep versions, or split the facade (`esp-sdk`) from the unstable crates?
3. Is `esp-sdk-build` + `board.dts` acceptable for customers, or should the TOML board format be
   offered as an alternative front-end?
4. Do we want a `no_std`-only policy forever, or an optional `std` feature for host-side testing of
   customer logic?
