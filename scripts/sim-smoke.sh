#!/usr/bin/env bash
# Boot a built SDK image in the esp32sim submodule.
#
# This is the emulator half of the "no hardware" loop: the emulator runs the
# real mask ROM, the real second-stage bootloader and our image, so it catches
# boot-path regressions before they reach a board.
#
# Until Phase 2 produces images, this script exits 0 with a message: a green CI
# run must never imply "verified in emulation" when nothing was booted.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SIM="$ROOT/third_party/esp32sim"
IMAGE_DIR="${1:-$ROOT/target/sim}"
SECONDS_TO_RUN="${SIM_SECONDS:-5}"

if [ ! -f "$SIM/Cargo.toml" ]; then
  echo "esp32sim submodule missing; run: git submodule update --init --depth 1" >&2
  exit 1
fi

for file in bootloader.bin partition-table.bin app.bin; do
  if [ ! -f "$IMAGE_DIR/$file" ]; then
    echo "no firmware image at $IMAGE_DIR/$file yet (Phase 2); skipping esp32sim boot"
    exit 0
  fi
done

ROM="${ROM_ELF:-$SIM/web/wasm/fw/esp32s3_rev0_rom.elf}"
if [ ! -f "$ROM" ]; then
  echo "mask ROM not found at $ROM" >&2
  echo "fetch it with: (cd third_party/esp32sim && tools/fetch-demo-assets.sh --no-linux)" >&2
  exit 1
fi

cargo build --release --manifest-path "$SIM/Cargo.toml"
"$SIM/target/release/esp32sim" \
  --boot rom --rom "$ROM" \
  --bootloader "$IMAGE_DIR/bootloader.bin" \
  --ptable "$IMAGE_DIR/partition-table.bin" \
  --app "$IMAGE_DIR/app.bin" \
  --max-seconds "$SECONDS_TO_RUN"
