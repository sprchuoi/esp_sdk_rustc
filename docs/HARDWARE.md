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

## Known issue (measured on hardware)

TX works: the banner, console help, timer line, I2C scan and scheduler output all
appear. RX does not yet: `--send` produces no echo. The raw UART0 FIFO read that
works in esp32sim is not sufficient on silicon because the bootloader leaves the
RX path unconfigured for this use. Next step is to switch `soc::Serial` RX to
`esp_hal::uart::Uart` (the REG_UPDATE handshake only stalls in esp32sim) or to
program CONF0/CONF1 explicitly.
