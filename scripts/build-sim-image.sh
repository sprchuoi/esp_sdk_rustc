#!/usr/bin/env bash
# Build an example and assemble the three images esp32sim boots:
#   target/sim/{bootloader.bin, partition-table.bin, app.bin}
#
# The app is placed at the factory partition (0x10000), which is where esp32sim
# writes --app. The bootloader is a standard ESP-IDF S3 build; it reads our
# partition table, so any valid IDF bootloader works. Override it with
# ESP_BOOTLOADER=/path/to/bootloader.bin.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXAMPLE="${1:-hello-kernel}"
OUT="${2:-$ROOT/target/sim}"
TARGET="xtensa-esp32s3-none-elf"
EXAMPLE_DIR="$ROOT/examples/$EXAMPLE"
ELF="$EXAMPLE_DIR/target/$TARGET/release/$EXAMPLE"

mkdir -p "$OUT"

if [ ! -f "$ELF" ]; then
  echo "building $EXAMPLE for $TARGET"
  (
    cd "$EXAMPLE_DIR"
    # shellcheck disable=SC1091
    . "$ROOT/scripts/xtensa-env.sh"
    cargo build --release --target "$TARGET"
  )
fi

echo "ELF -> ESP-IDF app image"
python3 -m esptool --chip esp32s3 elf2image \
  --flash-mode dio --flash-freq 80m --flash-size 16MB \
  -o "$OUT/app.bin" "$ELF"

echo "partition table (factory at 0x10000)"
python3 "$ROOT/scripts/gen-ptable.py" > "$OUT/partition-table.bin"

if [ -n "${ESP_BOOTLOADER:-}" ]; then
  BOOTLOADER="$ESP_BOOTLOADER"
else
  BOOTLOADER=""
  for candidate in \
    "$ROOT/boards/esp32s3-devkitc-1/bootloader.bin" \
    "$HOME/sandboxes/smart_home_idf/build/bootloader/bootloader.bin" \
    "$ROOT/third_party/esp32sim/web/wasm/fw/atech-bootloader.bin"; do
    if [ -f "$candidate" ]; then
      BOOTLOADER="$candidate"
      break
    fi
  done
fi

if [ -z "$BOOTLOADER" ] || [ ! -f "$BOOTLOADER" ]; then
  echo "no ESP-IDF S3 bootloader found." >&2
  echo "set ESP_BOOTLOADER=/path/to/bootloader.bin (or build one with ESP-IDF / espflash)" >&2
  exit 1
fi
cp "$BOOTLOADER" "$OUT/bootloader.bin"

ls -l "$OUT"
