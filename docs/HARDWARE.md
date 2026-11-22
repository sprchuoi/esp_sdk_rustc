# Hardware: flash and serial log

The SDK images in `target/sim/` are the same ones the emulator boots:

| Offset | Image |
|---|---|
| `0x0` | `bootloader.bin` (IDF second stage) |
| `0x8000` | `partition-table.bin` (factory @ `0x10000`) |
| `0x10000` | `app.bin` |

## Permissions

WSL attaches the CH343 as `/dev/ttyACM0`, usually `root:root 0600`:

```bash
sudo chmod 666 /dev/ttyACM0
# or, permanently:
sudo sh -c 'echo "KERNEL==\"ttyACM*\", MODE=\"0666\"" > /etc/udev/rules.d/99-esp32.rules'
sudo udevadm control --reload-rules && sudo udevadm trigger
```

`ESP_PORT=/dev/ttyUSB0` overrides autodetection.

## Flash

```bash
./esp_sdk.sh --flash              # 460800/921600 baud, three images
./esp_sdk.sh --flash-erase        # erase whole flash first
ESP_BAUD=460800 ./esp_sdk.sh --flash
```

This overwrites the vendor bootloader at `0x0`. Save a full dump first if the
board has to be restored:

```bash
python3 -m esptool --chip esp32s3 -p /dev/ttyACM0 read-flash 0 0x1000000 backup.bin
```

## Terminal log

```bash
./esp_sdk.sh --monitor            # 15 s, resets the board, saves target/serial.log
./esp_sdk.sh --monitor=30
./esp_sdk.sh --monitor=8 --send=$'help\nversion\n'
./esp_sdk.sh --hw                 # --flash then --monitor
```

`--monitor` toggles RTS/EN to reset the board, so the ROM and bootloader lines
are included. The raw capture is always written to `target/serial.log`; set
`ESP_NO_RESET=1` to listen without resetting, `ESP_MONITOR_BAUD` to change the
baud rate (default 115200).

## Board used

ESP32-S3, 16 MB flash, 8 MB octal PSRAM, CH343 (`1a86:55d3`), chip revision v0.2.
The factory image was at `0x20000` with OTA slots; the SDK image replaces the
partition table with a single `factory` app at `0x10000`.

## Checking an interrupt

The example has an `irq-selftest` feature: TIMG0 one-shot at 50 ms, `#[handler]`
clears `INT_CLR`, records `Instant`, and the main loop prints the delta. Build
with the feature, flash, and read the log:

```bash
ESP_FEATURES=irq-selftest ./esp_sdk.sh --build
ESP_BAUD=460800 ./esp_sdk.sh --flash --monitor=6
```

Measured on the attached board (rev v0.2):

```
irq: n=1 isr_at=217697us main_saw=217713us delta=16us
irq: n=2 isr_at=317895us main_saw=317903us delta=8us
irq: n=3 isr_at=418046us main_saw=418051us delta=5us
irq: n=4 isr_at=518196us main_saw=518201us delta=5us
irq: n=5 isr_at=618344us main_saw=618349us delta=5us
irq selftest: total=5
```

The interrupt fires, the handler returns, and the main loop observes it 5–16 µs
later. This delta is an upper bound: the main loop polls, and `Instant` is read
in the handler, so it does not include vector entry. The simulator's exception
storm with the same handler is therefore a simulator issue, not a firmware one.

## Known issue (measured on hardware)

TX works: the banner, console help, timer line, I2C scan and scheduler output all
appear. RX does not: `--send` produces no echo with either backend — the raw
UART0 FIFO read and the `hardware-uart` feature using `esp_hal::uart::Uart`.
Flashing over the same CH343 proves host→board data reaches the chip, so the
next checks are the UART0 pin mux / RX pin (GPIO44), `CONF0`/`CONF1`, and whether
the CH343's RX net is actually wired to U0RXD on this board. The command set is
verified in esp32sim meanwhile (`./esp_sdk.sh --console`).
