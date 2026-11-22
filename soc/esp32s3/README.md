# ESP32-S3 platform layer

The first concrete implementation of the SDK HAL (`hal::Serial`, `hal::Delay`).
Everything above this crate talks to `esp-sdk-hal` traits; `esp-hal` types stop
here.

```rust
let _peripherals = esp_hal::init(esp_hal::Config::default());
let mut serial = esp_sdk_esp32s3::Serial::uart0();
writeln!(serial, "Sprchuoi ESP32 Boot")?;
```

```bash
../../esp_sdk.sh --soc     # builds for xtensa-esp32s3-none-elf
```

## Status and known issue

- `Delay` wraps `esp_hal::delay::Delay` (busy-wait microseconds).
- `Serial` writes through esp-println's UART0 backend (GPIO43 TX / GPIO44 RX on
  the DevKitC-1). `hal::Serial::read` returns `Unsupported` until the RX
  interrupt path is wired.
- Direct `esp_hal::uart::Uart` construction currently stalls inside `Uart::new`
  under esp32sim (no output after the call). Until that is understood, the
  console uses esp-println's already-initialised UART backend. The HAL boundary
  is what the rest of the SDK depends on, so swapping the backend later is a
  change in this crate only — not in the kernel, console or example.
- GPIO, I2C, SPI, flash and the RWDT follow the same pattern (trait impls here,
  not in the kernel).
