# esp-sdk-hal

One module per peripheral, no catch-all file. Traits are re-exported at the crate
root, so drivers keep writing `esp_sdk_hal::I2cBus` and never depend on the file
layout.

| Module | Trait / types | Host mock |
|---|---|---|
| `delay.rs` | `Delay` | `mock/delay.rs` |
| `gpio.rs` | `GpioPin` | `mock/gpio.rs` |
| `i2c.rs` | `I2cBus` | `mock/i2c.rs` |
| `spi.rs` | `SpiBus` | `mock/spi.rs` |
| `serial.rs` | `Serial` | `mock/serial.rs` |
| `timer.rs` | `Timer` | `mock/timer.rs` |
| `flash.rs` | `Flash` | `mock/flash.rs` |
| `net.rs` | `NetDevice`, `MacAddr`, `LinkState`, `NetStats` | — (state machine in `net/`) |
| `rng.rs` | `Rng` | — |
| `error.rs` | `Error`, `Result` | — |

Adding a peripheral: one `foo.rs` with the trait, one `mock/foo.rs` for the host,
and one `pub use` line in `lib.rs`. Implementations live in `soc/`; the mock is
the only host-side code.
