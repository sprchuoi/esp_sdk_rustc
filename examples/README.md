# Examples

Runnable applications used to exercise the SDK on hardware and in the emulator.
They live outside the workspace (`exclude = ["examples"]`) because they depend on
`esp-hal` and only build for Xtensa.

| Example | What it shows |
|---|---|
| `hello-kernel` | boots an ESP32-S3, runs the TinyRTOS scheduler with three priority classes, moves sensor readings through an IPC queue, and prints a diagnostics dump every 5 s |

## `hello-kernel`

Prerequisites:

```bash
espup install --targets esp32s3          # Xtensa Rust toolchain
git submodule update --init --depth 1    # third_party/esp32sim
(cd third_party/esp32sim && tools/fetch-rom-elfs.sh web/wasm/fw)
```

Build the image set and boot it:

```bash
./scripts/build-sim-image.sh hello-kernel     # target/sim/{app,partition-table,bootloader}.bin
./scripts/sim-smoke.sh                        # boots esp32sim and asserts the output
```

Expected console (abridged; the `ESP-IDF ... 2nd stage bootloader` line comes from the purchased
bootloader, not from the app — see [docs/BOOT.md](../docs/BOOT.md#11-why-the-console-says-esp-idf-v551-2nd-stage-bootloader)):

```
I (7) boot: ESP-IDF v5.5.1 2nd stage bootloader
I (10) boot: Loaded app from partition at offset 0x10000
 esp-sdk hello-kernel on ESP32-S3
[ctrl] tick t=0 ms
[net ] publish sensor=0 t=0 ms
...
uptime_ms=5000 boot=1 health=ok
mem heap cap=65536 used=12288 free=53248 low=53248
metric mqtt.packets=9
metric sensor.reads=10
```

On a real board (once `soc/esp32s3` exists) the same app is flashed with
`espflash`, not through the emulator.
