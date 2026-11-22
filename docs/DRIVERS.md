# Driver Model and Storage/Network Subsystems

**Status:** draft v0.2 · 2026-09-21
How a driver is written, bound, powered and tested — plus the three subsystems this phase of the
SDK owes: **EEPROM**, **FLASH (MTD)** and **NET (WiFi)**. Companion to
[CONFIGURATION.md](CONFIGURATION.md).

**Implemented so far:**

- driver binding: `drivers/src/model.rs` (`DeviceNode`, `DriverDesc`, `bind`, unbound detection);
- AT24 I2C EEPROM: `drivers/src/eeprom/at24.rs` (page-aware writes, `tWR` wait, 1/2-byte addressing);
- mocks for every bus it needs: `hal/src/mock.rs`.

---

## 1. Driver model

Linux-like in spirit, smaller in machinery: no modules, no `ioctl`, no runtime discovery. Drivers
are linked in, matched against the board's device tree by **compatible string**, and probed once.

```rust
pub trait DriverFactory: Sync {
    /// e.g. &["atmel,at24c256", "atmel,at24c32"]
    const COMPATIBLE: &'static [&'static str];

    fn probe(&self, node: &DeviceNode, ctx: &mut ProbeCtx<'_>) -> Result<DeviceRef, ProbeError>;
}

pub enum DeviceRef {
    Eeprom(&'static dyn Eeprom),
    Mtd(&'static dyn Mtd),
    Net(&'static dyn NetDevice),
    Sensor(&'static dyn Sensor),
    Input(&'static dyn Input),
    Gpio(&'static dyn GpioPin),
    Pwm(&'static dyn Pwm),
    Adc(&'static dyn Adc),
}

pub enum ProbeError {
    Deferred,            // bus/clock/parent not ready — retried later, like -EPROBE_DEFER
    Unsupported(&'static str),
    Bus(BusError),
    Config(&'static str), // malformed node: missing property
}
```

**Registry.** `build.rs` generates the registry from the enabled Kconfig symbols:

```rust
// generated: esp_sdk_config::drivers
pub static DRIVERS: &[&dyn DriverFactory] = &[
    &at24::At24Factory, &w25q::W25qFactory, &bme280::Bme280Factory,
    &wifi::WifiFactory, &gpio_keys::GpioKeysFactory,
];
```

**Probe order.** Walk the device tree depth-first: buses before their children, SoC peripherals
before board devices. A driver that returns `Deferred` is retried (max 3 rounds) after the current
pass; anything still unbound is reported by the `devices` console command and, in CI defconfigs,
fails the build (strict binding).

**Device handles.** No heap, no trait-object maps: each probe writes into a `StaticCell` typed to the
concrete driver, and the board exposes named handles (`board.eeprom`, `board.flash`, `board.net`).
`DeviceRef` exists so the probe loop and diagnostics can stay generic.

**Rules for every driver**

- `#![forbid(unsafe_code)]`; all unsafe lives in `soc/` register glue.
- No panics: `Result` with a typed error, `unwrap` banned by clippy lint.
- No allocation; buffers are borrowed or moved in.
- No chip constants: pins, addresses, sizes and modes come from `DeviceNode`.
- No busy-waiting longer than a few microseconds; `await` for anything else.
- Every driver implements `suspend()`/`resume()` and `selftest()`.
- Every driver documents its ISR (if any) and its worst-case execution time.

**Power management.** `suspend` runs in reverse probe order, `resume` in probe order, both from the
kernel's sleep path. A driver that fails to suspend is skipped with a warning and counted — sleep
still happens, which is what a battery node needs.

---

## 2. Bus model

| Bus | Shape | Notes |
|---|---|---|
| I2C | one controller object per bus, shared through a short-lived async lock | 100/400 kHz, 7-bit addresses, clock-stretching timeout, bus recovery (9 clock pulses) after a stuck SDA |
| SPI | controller + per-device CS GPIO, mode/baud from the node | DMA above a threshold, CS asserted per transaction, no sharing between concurrent transactions |
| UART | split TX/RX, optional DMA ring | console and log sinks, framing errors surfaced not ignored |
| 1-Wire | bit-banged with timing from the SoC timer | DS18B20-class sensors; strict ISR masking windows |

Bus access is transactional: acquire, transfer with a timeout, release. Holding a bus across an
`.await` that depends on another device is a bug the lock timeout will expose.

---

## 3. EEPROM subsystem

Small non-volatile config stores: AT24Cxx on I2C, 25LCxxx on SPI, plus the emulated variant on flash
for boards with no EEPROM.

```rust
pub trait Eeprom: Send {
    fn capacity(&self) -> u32;
    fn page_size(&self) -> usize;
    async fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), EepromError>;
    async fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), EepromError>;
}
```

**AT24 (I2C)** — `atmel,at24c256` *(implemented)*:

- Address width follows capacity: ≤ 256 bytes uses one address byte, larger parts two. Derived from
  `size`, never hard-coded.
- Writes are split at page boundaries internally; a caller may pass any range.
- The write cycle (`tWR`, 5 ms typical) is honoured by `await`, not a spin loop. A write returns only
  after ACK polling (`RDSR`/dummy write) succeeds or the timeout (`CONFIG_EEPROM_WRITE_TIMEOUT_MS`)
  expires.
- `WP` pin, if wired, is driven inactive at probe and re-asserted on suspend.
- Read-back verification is available (`CONFIG_EEPROM_VERIFY`) — costs one extra read, catches
  marginal parts.

**25LC (SPI)** — `microchip,25lc256`-class:

- `WREN` → `WRITE` → poll `RDSR.WIP`; page size 16/32/64 from the node; status-register block
  protection cleared at probe unless the board says otherwise.

**EEPROM-backed KV.** Boards with no spare flash get `EepromKv`: a log-structured ring across pages
with sequence numbers and CRC, compaction when full. 1 M write cycles per cell is the budget; the
config is written on change, not periodically, and the ring spreads writes across pages.

**Errors.** NACK, timeout, verify mismatch and out-of-range are distinct variants. Two consecutive
failures mark the device degraded (read-only) and raise a diagnostic; the board keeps running.

---

## 4. FLASH / MTD subsystem

The on-chip SPI NOR (16 MB on the N16R8) is the primary store: partitions, KV, forensic log, OTA
slots. External SPI NOR/NAND plugs into the same interface later.

```rust
pub trait Mtd: Send {
    fn size(&self) -> u64;
    fn erase_size(&self) -> u32;   // 4 KiB on SPI NOR
    fn write_size(&self) -> u32;   // 1..256 bytes (page program)
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), MtdError>;
    fn erase(&mut self, offset: u64, len: u64) -> Result<(), MtdError>;
    fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), MtdError>;
    fn jedec_id(&mut self) -> Result<[u8; 3], MtdError>;
    fn is_bad_block(&self, block: u32) -> Result<bool, MtdError>; // false on NOR, real on NAND
}
```

**Rules that prevent field failures**

- Erase before write; a write to a non-erased region is an error, not silent corruption.
- Partition handles are range-checked: `MtdPartition<'a>` refuses any access outside its range, so a
  bug cannot erase the bootloader or the partition table.
- `otadata` is written only through `esp-sdk-boot`, never by a driver.
- No flash writes from an ISR. Writes run in the `App` executor with the cache disabled by the SoC
  layer (code executing from flash stalls — this is why OTA writes are chunked and yield).
- Wear accounting: erase count per block, program/erase error counters, exposed as diagnostics.
- KV and log are log-structured with compaction; that is what keeps erase counts low enough for a
  10-year node.

**Layers above MTD**

```
board.dts partitions
        │
   MtdPartition (range-checked)
        ├── kv        log-structured, atomic commit, AES-GCM sealed secrets
        ├── log       circular defmt ring (forensics)
        └── ota_0/1   image stream + SHA-256 + Ed25519, otadata marking
```

`embedded-storage::NorFlash` is implemented as an adapter so `littlefs2` (or any other
`embedded-storage` consumer) can be added later without touching the MTD core. **No general-purpose
filesystem in v1** — KV + log + OTA cover the product; a filesystem is a Phase 6 decision, and the
adapter keeps that door open.

---

## 5. NET subsystem

Linux has `netdev`; we keep the concept, not the syscall surface.

```rust
pub trait NetDevice: Send + Sync {
    fn name(&self) -> &'static str;
    fn mtu(&self) -> usize;
    fn mac(&self) -> [u8; 6];
    async fn up(&mut self) -> Result<(), NetError>;
    async fn down(&mut self);
    fn link(&self) -> LinkState;          // Down, Up { rssi: i8 }
    fn stats(&self) -> NetStats;          // rx/tx pkts, bytes, errors, drops
}
```

**WiFi (`espressif,esp32s3-wifi`, via `esp-radio`)** — the first netdev:

- STA with optional SoftAP during provisioning; scan, join, RSSI, disconnect reason.
- Power save: modem sleep configurable (`CONFIG_NET_WIFI_PS`), off in `Mains/AP` mode, on for
  `Battery`.
- Reconnect is supervised by the WiFi manager, not the driver: driver reports events, service owns
  the state machine and backoff.
- Stats feed diagnostics: RSSI, retries, disconnects, bytes.
- Credentials never in Kconfig; they live in KV, provisioned over serial or the portal.

**Stack.** One `embassy-net` interface per netdev, DHCP client (static IP from KV as an override),
SNTP for wall clock. Applications get `embedded-io-async` streams — MQTT and the provisioning HTTP
server use those, not sockets. There is no POSIX layer and no lwIP.

**Portability demo.** `w5500` SPI Ethernet is the second netdev implementing the same trait; if the
WiFi service code compiles untouched against it, the split between link and stack is real.

**Provisioning portal.** SoftAP + a small HTTP server on `embassy-net`: `GET /` status page,
`POST /wifisave` (`s=`, `p=`) — compatible with the bench harness already used for the C++ node.

---

## 6. Testing per class

| Class | Host test | On target |
|---|---|---|
| Bus | mock controller, timeout/NAK injection | `i2c scan`, stuck-bus recovery |
| EEPROM | fake with page semantics + 5 ms write delay | read/write/verify, power-cut mid-write |
| MTD | fake flash with byte-granular power cut | erase/program, range guards, OTA stream, wear counters |
| NET | fake link with loss/latency | join/leave, AP flap, broker restart, RSSI sanity |
| Driver core | probe order, deferred probe, unbound-node failure | `devices` command, strict CI binding |

`selftest` runs the on-target column from the console and publishes a pass/fail diagnostic.

---

## 7. Rejected patterns

- **Runtime driver loading / hotplug** — no modules; every driver is linked and matched at boot.
- **`dyn Any` downcasting everywhere** — typed handles and a per-class `DeviceRef`.
- **Drivers reading Kconfig for wiring** — wiring is in the device tree.
- **Blocking sleeps in buses** — `await`, and timeouts from the kernel timer heap.
- **Direct flash access from services** — everything goes through `MtdPartition`, with bounds.
- **A filesystem on day one** — KV + log + OTA first; littlefs only if a real need appears.
