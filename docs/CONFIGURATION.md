# Configuration and Portability

**Status:** draft v0.1 · 2026-09-21
How the SDK stays one source tree for many chips, boards and products — the Linux-kernel model,
adapted to Rust. Companion to [PLAN.md](PLAN.md) and [DRIVERS.md](DRIVERS.md).

---

## 1. The goal

Linux is portable because the *kernel* is configuration-independent and the *machine* is described
in data, not code. This SDK copies that split:

| Layer | Linux | This SDK |
|---|---|---|
| Feature/policy selection | `Kconfig` → `.config` | `Kconfig` → `.config` → generated Rust |
| Hardware description | `.dts` → `.dtb` at build time | `.dts` → `dtc` → `.dtb` → build-time parser |
| Driver selection | `compatible = "vendor,chip"` + `of_match_table` | same idea: compatible strings + `DriverFactory` registry |
| Machine code | `arch/`, `soc/` | `arch/`, `soc/` |
| Board support | `arch/arm/boot/dts/*.dts` | `boards/<board>/board.dts` |
| Product | defconfig | board/app defconfig |

Consequences we want:

- Adding a **board** is data: pins, buses, devices and partitions in one `.dts`, no Rust.
- Adding a **driver** is one module + one Kconfig symbol; it binds itself by `compatible`.
- Disabling a feature **removes** its code: no runtime `if`, no dead flash bytes.
- CI compiles `defconfig`s *and* `allyesconfig`, so disabled configurations cannot rot.
- Nothing is discovered at runtime that could be fixed at build time.

Consequences we accept:

- A build step (`xtask` → kconfiglib + dtc + codegen) is mandatory. Plain `cargo build` is not the
  interface; `cargo xtask build` is.
- `dtc` and Python are build-host dependencies (both present on this machine: DTC 1.7.0, Python 3.12).

---

## 2. Three inputs, one generated crate

```
Kconfig (features, SoC, board, app)      boards/<b>/board.dts (hardware tree)
            │                                         │
            ▼                                         ▼
      .config (kconfiglib)                    board.dtb (dtc)
            │                                         │
            └──────────► tools/genconfig ◄────────────┘
                              │
                              ▼
                 esp-sdk-config (generated crate)
                 ├── sdkconfig.rs   pub const CONFIG_*: ...
                 ├── board.rs       BOARD, DEVICES, PARTITIONS
                 └── cfgs.rs        #[cfg(drv_at24)] ... plumbing
```

- `esp-sdk-config` is generated into `target/<triple>/.../gen/` by `xtask`, not committed.
- Firmware reads `esp_sdk_config::CONFIG_WIFI` (a `const bool`, folded by the optimiser).
- Cargo features are **not** the product interface. They are reserved for `host-test` and
  `unstable`; everything a product chooses is a Kconfig symbol. One mechanism, not two.

---

## 3. Kconfig layout

```
Kconfig                        # top level, sources the rest
├── arch/Kconfig               # ARCH_XTENSA / ARCH_RISCV
├── soc/Kconfig                # SOC_ESP32S3 / SOC_ESP32C6 (+ peripherals they provide)
├── drivers/Kconfig
│   ├── drivers/bus/Kconfig    # I2C, SPI, UART, 1-Wire
│   ├── drivers/eeprom/Kconfig # DRV_EEPROM_AT24, DRV_EEPROM_25LC
│   ├── drivers/mtd/Kconfig    # MTD_SPI_NOR, MTD_SPI_NOR_SOC
│   ├── drivers/net/Kconfig    # NET_WIFI, NET_ETH_W5500
│   ├── drivers/gpio/Kconfig   # GPIO, PWM, ADC, input
│   └── drivers/sensor/Kconfig
├── subsys/Kconfig             # NET, MQTT, STORAGE_KV, OTA, SMARTHOME
├── boards/Kconfig             # BOARD_DEVKITC_1, BOARD_DEVKITC_C6, BOARD_CUSTOM
└── apps/Kconfig               # APP_SENSOR_NODE, APP_ACTUATOR_NODE
```

Symbol conventions:

| Prefix | Meaning | Example |
|---|---|---|
| `SOC_*` | chip selected by the board | `CONFIG_SOC_ESP32S3=y` |
| `ARCH_*` | CPU family, implied by SoC | `CONFIG_ARCH_XTENSA=y` |
| `BOARD_*` | board support | `CONFIG_BOARD_DEVKITC_1=y` |
| `DRV_*` | a driver | `CONFIG_DRV_EEPROM_AT24=y` |
| `SUBSYS_*` | kernel/service subsystem | `CONFIG_SUBSYS_MQTT=y` |
| `APP_*` | the product | `CONFIG_APP_SENSOR_NODE=y` |

Example — `drivers/eeprom/Kconfig`:

```kconfig
menuconfig DRV_EEPROM
    bool "EEPROM support"
    default y
    help
      Generic EEPROM device class (I2C and SPI parts). Used for small
      configuration stores on boards with no spare flash.

if DRV_EEPROM

config DRV_EEPROM_AT24
    bool "AT24Cxx I2C EEPROM"
    depends on I2C
    default y

config DRV_EEPROM_25LC
    bool "25LCxxx SPI EEPROM"
    depends on SPI
    default n

config EEPROM_WRITE_TIMEOUT_MS
    int "Write-cycle timeout (ms)"
    range 1 50
    default 10

endif
```

Rules (learned from Linux/Zephyr):

- **No hardware facts in Kconfig.** Pin numbers, addresses and sizes belong in the board `.dts`.
  Kconfig decides *whether* a driver exists, not *where* it is wired.
- **`select` sparingly.** Prefer `depends on`; a `select` chain is how Kconfig trees become
  unbuildable.
- Every symbol has `help` text; `xtask kconfig lint` fails on missing help or dead symbols.
- Values are compile-time. If a value must change at runtime (WiFi credentials, MQTT host), it is
  **not** a Kconfig symbol — it is KV data.

---

## 4. Board description (DTS)

`boards/esp32s3-devkitc-1/board.dts`:

```dts
/dts-v1/;

/ {
    compatible = "sprchuoi,esp32s3-devkitc-1";
    model = "ESP32-S3-DevKitC-1 (N16R8)";

    chosen { stdout-path = &uart0; };
    aliases { serial0 = &uart0; };

    uart0: serial@60000000 {
        compatible = "espressif,esp32s3-uart";
        reg = <0x60000000 0x1000>;
        status = "okay";
    };

    i2c0: i2c@60004000 {
        compatible = "espressif,esp32s3-i2c";
        reg = <0x60004000 0x1000>;
        clock-frequency = <400000>;
        status = "okay";

        eeprom@50 {
            compatible = "atmel,at24c256";
            reg = <0x50>;
            size = <32768>;          /* bytes */
            pagesize = <64>;
        };

        sensor@76 {
            compatible = "bosch,bme280";
            reg = <0x76>;
        };
    };

    led0: led {
        compatible = "gpio-led";
        gpios = <&gpio0 2 0>;        /* controller, pin, flags */
        label = "status";
    };

    button0: button {
        compatible = "gpio-keys";
        gpios = <&gpio0 0 1>;        /* active low */
        label = "boot";
    };

    partitions {
        compatible = "esp-sdk,partitions";
        ota_0 { label = "ota_0"; reg = <0x20000 0x400000>; };
        ota_1 { label = "ota_1"; reg = <0x420000 0x400000>; };
        kv    { label = "kv";    reg = <0x820000 0x20000>; };
        log   { label = "log";   reg = <0x840000 0x10000>; };
    };
};
```

Build pipeline:

```
cpp -P -x assembler-with-cpp board.dts | dtc -I dts -O dtb -o board.dtb
```

`cpp` gives us `#include` and `#define` for shared fragments (SoC `.dtsi`), exactly as in Linux.
`dtc` is already installed here (1.7.0). The build step then parses the DTB in `build.rs`
(host-side, `fdt` crate) and emits `board.rs`:

```rust
pub static BOARD: Board = Board {
    name: "ESP32-S3-DevKitC-1 (N16R8)",
    soc: Soc::Esp32S3,
    serial: Some(SerialId::Uart0),
    devices: &[ /* one entry per enabled node */ ],
    partitions: &[ /* typed partition table */ ],
};
```

Validation at build time, not at 3 a.m. on a device:

- every `compatible` must have an enabled driver, otherwise **build error** listing the node path
  (Linux prints "no driver found" at runtime; we can do better);
- every `reg` of an I2C device must be 7-bit and unique on the bus;
- a referenced `&gpio0`/`&i2c0` must exist; pin conflicts are reported;
- partition ranges must fit the flash size and must not overlap;
- `status = "disabled"` removes a node from the generated table.

---

## 5. Build flow

```bash
cargo xtask menuconfig --board devkitc1        # ncurses TUI, writes .config
cargo xtask savedefconfig                      # .config -> boards/.../defconfig
cargo xtask build --app sensor-node            # codegen + cargo build for the selected SoC
cargo xtask flash --app sensor-node
cargo xtask ci                                 # matrix (see §7)
```

- `tools/kconfig/` vendors `kconfiglib.py` (ISC, Ulf Magnusson) plus `menuconfig.py`; no pip
  dependency at build time. A native Rust Kconfig parser can replace the front-end later without
  changing the `.config` format or the generator.
- `.config` is **not** committed; `*_defconfig` is (Linux convention).
- `xtask` resolves `CONFIG_SOC_*` → target triple + linker script + rustflags, then invokes Cargo
  once with `SDKCONFIG=<path>`; each crate's `build.rs` consumes it.
- Reproducibility: `.config` hash, compiler version, `esp-hal` version and git SHA are stamped into
  the app descriptor and printed by `xtask version`.

### Generated `sdkconfig.rs`

```rust
pub const CONFIG_SOC_ESP32S3: bool = true;
pub const CONFIG_ARCH_XTENSA: bool = true;
pub const CONFIG_DRV_EEPROM_AT24: bool = true;
pub const CONFIG_EEPROM_WRITE_TIMEOUT_MS: u32 = 10;
pub const CONFIG_LOG_LEVEL: u8 = 3;
pub const CONFIG_SUBSYS_MQTT: bool = true;
```

- Constants for values, `#[cfg]` for whole modules.
- `build.rs` emits `cargo:rustc-check-cfg=cfg(drv_at24, ...)`; crates use
  `#![deny(unexpected_cfgs)]`, so a typo is a build failure rather than a silently dead branch.
- A helper macro keeps `#[cfg]` noise out of the code: `cfg_mod!(drv_at24, mod at24);`.

---

## 6. Porting recipes

### Add a SoC (e.g. ESP32-C6)

```
soc/esp32c6/
├── Cargo.toml            # esp-hal with the right chip feature
├── target.rs             # riscv32imac-unknown-none-elf, linker script, panic/alloc setup
├── clock.rs, gpio.rs, i2c.rs, spi.rs, uart.rs, dma.rs, flash.rs, wifi.rs, timer.rs
└── Kconfig               # SOC_ESP32C6 + the peripherals it provides
```

Checklist: implements `Arch` + the `soc::Peripherals` constructor; adds a `target` mapping in `xtask`;
no changes to `kernel/`, `drivers/` or `subsys/`. If a driver needs a hook the SoC does not provide,
the trait gains an `Option`/`Result::Unsupported` — never a `#[cfg(soc)]` in a driver.

### Add a board

One directory, zero Rust:

```
boards/acme-plug/
├── board.dts        # SoC include, buses, devices, partitions, pins
└── defconfig        # CONFIG_SOC_*=..., CONFIG_* driver selection, CONFIG_APP_*
```

### Add a driver

```
drivers/<class>/<name>/
├── Cargo.toml
├── src/lib.rs       # DriverFactory with compatible strings + probe
└── Kconfig          # DRV_<CLASS>_<NAME>
```

Checklist: no chip numbers (use the `DeviceNode`), no global state (use `ProbeCtx`), no panics, no
allocation, probe returns `ProbeError::Deferred` if a dependency is not ready. `docs/DRIVERS.md` has
the trait definitions.

---

## 7. CI matrix

| Configuration | Purpose | Build | Run |
|---|---|---|---|
| `devkitc1_sensor_defconfig` | primary product | yes | HIL |
| `devkitc1_actuator_defconfig` | actuators, PWM, buttons | yes | HIL |
| `devkitc6_sensor_defconfig` | RISC-V portability | yes | HIL when board present |
| `allyesconfig` | every driver/subsystem compiles together | yes | — |
| `allnoconfig` | kernel-only build (no drivers, no net) still links | yes | host tests |
| host `cargo test` | pure logic + mocks | yes | CI |
| size budget per defconfig | flash/RAM regression | yes | CI |

`allyesconfig` is the cheap insurance that keeps disabled drivers from rotting — it is the single
most valuable habit borrowed from the Linux build.

---

## 8. Downstream consumers (the published SDK)

Inside this repo, Kconfig is authoritative. A **published crate** cannot be: a crates.io consumer
must build from `Cargo.toml` + `Cargo.lock` alone, and a library must never read a project-global
`.config` (it breaks docs.rs, reproducible builds and every consumer that forgets the file).

So the boundary is:

| Layer | SDK repo | Customer project |
|---|---|---|
| Selection | Kconfig + defconfig | Cargo features (small curated set) |
| Hardware | `boards/*/board.dts` | their own `board.dts` |
| Codegen | `xtask build` | `esp-sdk-build` in `build.rs` |
| Convenience | `menuconfig` | `cargo esp-sdk menuconfig` → writes the feature set, then `cargo esp-sdk build` |

`cargo esp-sdk` uses the same Kconfig definitions shipped as data, computes the feature set and
invokes Cargo with `--features`; the customer's `Cargo.toml` stays the source of truth for a plain
`cargo build`. `esp-sdk-build` validates that every `compatible` in the board file has an enabled
driver and names the missing feature in the error.

Full contract: [SDK.md](SDK.md).

## 9. Anti-patterns (rejected by design)

- **A global `.config` consumed by a published library** — unbuildable for consumers, and
  unreproducible. Features are the compile-time contract; Kconfig is the authoring front-end.
- **A feature per driver** — the combination space explodes. Features are per *class*
  (`eeprom`, `flash`, `wifi`), and the board file validates what is actually needed.
- **Pin numbers in drivers or apps** — pins are board data.
- **Runtime probing/discovery of hardware we already know** — the board file is the truth; a missing
  device is a build error.
- **`#[cfg]` soup inside functions** — cfg at module boundaries only, via the generated `cfgs.rs`.
- **Secrets or site configuration in Kconfig** — credentials belong in KV, never in a build artifact
  that gets committed or shared.
- **Kconfig for runtime-varying values** — baud rates chosen by a user at runtime are KV data, not
  `CONFIG_*`.

---

## 10. Open points

1. kconfiglib vendoring vs a native Rust parser (front-end swap is cheap; the format stays).
2. DTS-`dtc` (chosen) vs TOML manifest. DTS buys overlays, `cpp` includes, tooling and familiarity;
   TOML would drop the `dtc`/`fdt` dependency. Mitigation: both front-ends can emit the same
   `BoardIR` if the TOML path is ever needed.
3. Whether to ship board variants as overlays (`board + board-plug-overlay`) or separate defconfigs.
4. `menuconfig` TUI: kconfiglib's ncurses UI now, a `ratatui` UI later if the Python dependency
   becomes annoying.
