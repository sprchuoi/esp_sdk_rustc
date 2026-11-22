#!/usr/bin/env bash
# Build the B0 bootloader and prove the ROM loads and runs it in esp32sim.
# B0 prints a banner from IRAM and halts; it does not boot an app yet.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SIM="$ROOT/third_party/esp32sim"
SRC="$ROOT/bootloader"
OUT="$ROOT/target/sim"
TARGET="xtensa-esp32s3-none-elf"
ELF="$SRC/target/$TARGET/release/esp-sdk-bootloader"
ROM="${ROM_ELF:-$SIM/web/wasm/fw/esp32s3_rev0_rom.elf}"

mkdir -p "$OUT"

(
  cd "$SRC"
  # shellcheck disable=SC1091
  . "$ROOT/scripts/xtensa-env.sh"
  cargo build --release --target "$TARGET"
)

echo "ELF -> ESP image (entry is our _start in IRAM)"
python3 -m esptool --chip esp32s3 elf2image \
  --flash-mode dio --flash-freq 80m --flash-size 8MB \
  -o "$OUT/bootloader-b0.bin" "$ELF"

if [ ! -x "$SIM/target/release/esp32sim" ]; then
  cargo build --release --bin esp32sim --manifest-path "$SIM/Cargo.toml"
fi

LOG="$(mktemp)"
trap 'rm -f "$LOG"' EXIT

"$SIM/target/release/esp32sim" \
  --chip s3 \
  --boot rom \
  --rom "$ROM" \
  --bootloader "$OUT/bootloader-b0.bin" \
  --console uart0 \
  --max-seconds 1 \
  --flash-mb 8 | tee "$LOG"

grep -q "load:0x403c0000" "$LOG" ||
  { echo "FAIL: ROM did not load our IRAM segment" >&2; exit 1; }
grep -q "Sprchuoi ESP32 Boot" "$LOG" ||
  { echo "FAIL: product banner not printed" >&2; exit 1; }
grep -q "running from IRAM" "$LOG" ||
  { echo "FAIL: B0 stage line not printed" >&2; exit 1; }

echo "B0 OK: mask ROM loaded our image and ran our code from IRAM"
