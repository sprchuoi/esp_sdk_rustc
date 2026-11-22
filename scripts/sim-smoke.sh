#!/usr/bin/env bash
# Boot the hello-kernel example in esp32sim and check that the kernel runs.
#
# This is the headless regression net for the boot path: real mask ROM, real
# ESP-IDF second-stage bootloader, our partition table and our app image.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SIM="$ROOT/third_party/esp32sim"
IMAGE_DIR="${1:-$ROOT/target/sim}"
SECONDS_TO_RUN="${SIM_SECONDS:-6}"
ROM="${ROM_ELF:-$SIM/web/wasm/fw/esp32s3_rev0_rom.elf}"

if [ ! -f "$SIM/Cargo.toml" ]; then
  echo "esp32sim submodule missing; run: git submodule update --init --depth 1" >&2
  exit 1
fi

if [ ! -f "$IMAGE_DIR/app.bin" ]; then
  "$ROOT/scripts/build-sim-image.sh" hello-kernel "$IMAGE_DIR"
fi

if [ ! -f "$ROM" ]; then
  echo "mask ROM not found at $ROM" >&2
  echo "fetch it with: (cd third_party/esp32sim && tools/fetch-rom-elfs.sh web/wasm/fw)" >&2
  exit 1
fi

if [ ! -x "$SIM/target/release/esp32sim" ]; then
  cargo build --release --bin esp32sim --manifest-path "$SIM/Cargo.toml"
fi

LOG="$(mktemp)"
trap 'rm -f "$LOG"' EXIT

"$SIM/target/release/esp32sim" \
  --chip s3 \
  --boot rom \
  --rom "$ROM" \
  --bootloader "$IMAGE_DIR/bootloader.bin" \
  --ptable "$IMAGE_DIR/partition-table.bin" \
  --app "$IMAGE_DIR/app.bin" \
  --console uart0 \
  --max-seconds "$SECONDS_TO_RUN" \
  --flash-mb 8 --psram-mb 8 | tee "$LOG"

grep -q "Sprchuoi ESP32 Boot" "$LOG" ||
  { echo "FAIL: product banner not printed by the app" >&2; exit 1; }
grep -q "esp-sdk hello-kernel on ESP32-S3" "$LOG" ||
  { echo "FAIL: kernel banner not reached" >&2; exit 1; }
grep -q "\[ctrl\] tick" "$LOG" ||
  { echo "FAIL: scheduler did not run the control task" >&2; exit 1; }
grep -q "uptime_ms=" "$LOG" ||
  { echo "FAIL: diagnostics dump not produced" >&2; exit 1; }
grep -q "metric mqtt.packets=" "$LOG" ||
  { echo "FAIL: IPC traffic not recorded" >&2; exit 1; }

echo "SIM OK: kernel booted, scheduled, moved data over IPC and dumped health"
