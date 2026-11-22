#!/usr/bin/env bash
# esp_sdk.sh — one entry point for the SDK repository.
#
# Host checks use the stable toolchain; --build/--bootloader/--sim* use the
# espup Xtensa toolchain (found automatically, see setup_xtensa).
#
#   ./esp_sdk.sh --docs --test        (any number of commands, in order)
#   ./esp_sdk.sh --check              fmt + clippy + tests + docs
#   ./esp_sdk.sh --build --sim        build the example and boot it in esp32sim
#   ./esp_sdk.sh --bootloader --sim-b0
#   ./esp_sdk.sh --all                check + build + image + both sim runs
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

SIM="$ROOT/third_party/esp32sim"
OUT="$ROOT/target/sim"
TARGET="xtensa-esp32s3-none-elf"
EXAMPLE="hello-kernel"
ROM="$SIM/web/wasm/fw/esp32s3_rev0_rom.elf"
TMP_LOG="$(mktemp)"
trap 'rm -f "$TMP_LOG"' EXIT

log() { printf '\n==> %s\n' "$*"; }
die() { echo "error: $*" >&2; exit 1; }

usage() {
  cat <<'USAGE'
esp_sdk.sh — build, test and emulate the SDK

Host (stable Rust):
  --fmt         cargo fmt --check
  --lint        cargo clippy (deny warnings)
  --test        cargo test --workspace
  --docs        cargo doc (deny warnings)
  --nostd       build the workspace for x86_64-unknown-none
  --check       --fmt --lint --test --docs

Device (Xtensa toolchain):
  --build       build examples/hello-kernel and assemble target/sim/*
  --soc         build the ESP32-S3 platform crate
  --bootloader  build the B0 bootloader and target/sim/bootloader-b0.bin
  --sim         boot the example in esp32sim (asserts)
  --console     drive the interactive console with scripted UART input
  --sim-b0      boot the B0 bootloader in esp32sim (asserts)
  --all         --check --soc --build --bootloader --sim --console --sim-b0

  --help        this text
USAGE
}

setup_xtensa() {
  local export_file="${ESPUP_EXPORT_FILE:-$HOME/export-esp.sh}"
  if [ -f "$export_file" ]; then
    set -a
    # shellcheck disable=SC1090
    . "$export_file"
    set +a
    return
  fi
  local dir
  for dir in "$HOME"/.rustup/toolchains/esp/xtensa-esp-elf/*/xtensa-esp-elf/bin; do
    [ -d "$dir" ] && PATH="$dir:$PATH"
  done
  for dir in "$HOME"/.rustup/toolchains/esp/xtensa-esp-elf-clang/*/esp-clang/lib; do
    [ -d "$dir" ] && LIBCLANG_PATH="$dir"
  done
  export PATH LIBCLANG_PATH
  command -v xtensa-esp32s3-elf-gcc >/dev/null 2>&1 ||
    die "Xtensa toolchain not found; run: espup install --targets esp32s3"
}

ensure_sim_binary() {
  [ -d "$SIM/Cargo.toml" ] 2>/dev/null || [ -f "$SIM/Cargo.toml" ] ||
    die "esp32sim submodule missing; run: git submodule update --init --depth 1"
  if [ ! -x "$SIM/target/release/esp32sim" ]; then
    log "building esp32sim (release)"
    cargo build --release --bin esp32sim --manifest-path "$SIM/Cargo.toml"
  fi
}

ensure_rom() {
  if [ ! -f "$ROM" ]; then
    log "fetching mask ROMs"
    ( cd "$SIM" && tools/fetch-rom-elfs.sh web/wasm/fw )
  fi
}

gen_partition_table() {
  mkdir -p "$OUT"
  python3 - "$OUT/partition-table.bin" <<'PY'
import hashlib, struct, sys
MAGIC, MD5_MAGIC = 0x50AA, 0xEBEB
entries = [
    ("nvs", 1, 2, 0x9000, 0x6000, 0),
    ("phy_init", 1, 1, 0xF000, 0x1000, 0),
    ("factory", 0, 0, 0x10000, 0x300000, 0),
]
table = bytearray()
for label, ty, sub, off, size, flags in entries:
    table += struct.pack("<HBBII16sI", MAGIC, ty, sub, off, size,
                         label.encode().ljust(16, b"\0"), flags)
table += struct.pack("<H", MD5_MAGIC) + b"\0" * 14 + hashlib.md5(table).digest()
table += b"\xff" * (0xC00 - len(table))
with open(sys.argv[1], "wb") as out:
    out.write(table)
PY
}

elf2image() { # ELF OUTPUT FLASH_SIZE
  python3 -m esptool --chip esp32s3 elf2image \
    --flash-mode dio --flash-freq 80m --flash-size "$3" \
    -o "$2" "$1" >/dev/null
}

find_bootloader() {
  local candidate
  for candidate in \
    "${ESP_BOOTLOADER:-}" \
    "$ROOT/boards/esp32s3-devkitc-1/bootloader.bin" \
    "$HOME/sandboxes/smart_home_idf/build/bootloader/bootloader.bin" \
    "$SIM/web/wasm/fw/atech-bootloader.bin"; do
    if [ -n "$candidate" ] && [ -f "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

# --- host -----------------------------------------------------------------
cmd_fmt()  { log "rustfmt"; cargo fmt --all -- --check; }
cmd_lint() { log "clippy"; cargo clippy --workspace --all-targets --all-features -- -D warnings; }
cmd_test() { log "host tests"; cargo test --workspace --all-features; }
cmd_docs() { log "rustdoc"; RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features; }
cmd_nostd() {
  log "no_std build (x86_64-unknown-none)"
  rustup target add x86_64-unknown-none >/dev/null 2>&1 || true
  cargo check --workspace --all-features --target x86_64-unknown-none
}

# --- device ---------------------------------------------------------------
cmd_build() {
  setup_xtensa
  log "building examples/$EXAMPLE for $TARGET"
  ( cd "$ROOT/examples/$EXAMPLE" && cargo build --release --target "$TARGET" )
  mkdir -p "$OUT"
  elf2image "$ROOT/examples/$EXAMPLE/target/$TARGET/release/$EXAMPLE" "$OUT/app.bin" 16MB
  gen_partition_table
  local bootloader
  bootloader="$(find_bootloader)" || die "no ESP-IDF S3 bootloader found; set ESP_BOOTLOADER"
  cp "$bootloader" "$OUT/bootloader.bin"
  log "image ready in target/sim"
}

cmd_soc() {
  setup_xtensa
  log "building soc/esp32s3"
  ( cd "$ROOT/soc/esp32s3" && cargo build --release --target "$TARGET" )
}

cmd_bootloader() {
  setup_xtensa
  log "building B0 bootloader"
  ( cd "$ROOT/bootloader" && cargo build --release --target "$TARGET" )
  mkdir -p "$OUT"
  elf2image "$ROOT/bootloader/target/$TARGET/release/esp-sdk-bootloader" \
    "$OUT/bootloader-b0.bin" 8MB
}

run_sim() { # args passed to esp32sim
  ensure_sim_binary
  ensure_rom
  "$SIM/target/release/esp32sim" --chip s3 --boot rom --rom "$ROM" \
    --console uart0 --max-seconds "${SIM_SECONDS:-6}" --flash-mb 8 "$@" | tee "$TMP_LOG"
}

cmd_sim() {
  [ -f "$OUT/app.bin" ] || cmd_build
  log "booting $EXAMPLE in esp32sim"
  run_sim --bootloader "$OUT/bootloader.bin" --ptable "$OUT/partition-table.bin" \
    --app "$OUT/app.bin" --psram-mb 8
  grep -q "Sprchuoi ESP32 Boot" "$TMP_LOG" ||
    die "product banner missing"
  grep -q "esp-sdk hello-kernel on ESP32-S3" "$TMP_LOG" ||
    die "kernel banner missing"
  grep -q "\[ctrl\] tick" "$TMP_LOG" || die "scheduler did not run"
  grep -q "uptime_ms=" "$TMP_LOG" || die "diagnostics dump missing"
  grep -q "metric mqtt.packets=" "$TMP_LOG" || die "IPC traffic missing"
  echo "SIM OK: boot, scheduler, IPC and diagnostics verified"
}

cmd_console() {
  [ -f "$OUT/app.bin" ] || cmd_build
  local script
  script="$(mktemp)"
  printf '0.3 uart0 help\n0.8 uart0 version\n' > "$script"
  log "driving the console over UART0"
  run_sim --bootloader "$OUT/bootloader.bin" --ptable "$OUT/partition-table.bin" \
    --app "$OUT/app.bin" --psram-mb 8 --script "$script"
  rm -f "$script"
  grep -q "commands:" "$TMP_LOG" || die "console help missing"
  grep -q "hello-kernel 0.1.0" "$TMP_LOG" || die "console command did not run"
  echo "CONSOLE OK: UART0 input, line echo and command dispatch"
}

cmd_sim_b0() {
  [ -f "$OUT/bootloader-b0.bin" ] || cmd_bootloader
  log "booting B0 bootloader in esp32sim"
  run_sim --bootloader "$OUT/bootloader-b0.bin" --max-seconds 1 --flash-mb 8
  grep -q "load:0x403c0000" "$TMP_LOG" || die "ROM did not load our IRAM segment"
  grep -q "Sprchuoi ESP32 Boot" "$TMP_LOG" || die "B0 banner missing"
  grep -q "running from IRAM" "$TMP_LOG" || die "B0 stage line missing"
  echo "B0 OK: ROM loaded and ran our image"
}

# --- dispatch -------------------------------------------------------------
[ $# -gt 0 ] || { usage; exit 2; }

for arg in "$@"; do
  case "$arg" in
    --fmt)        cmd_fmt ;;
    --lint)       cmd_lint ;;
    --test)       cmd_test ;;
    --docs)       cmd_docs ;;
    --nostd)      cmd_nostd ;;
    --check)      cmd_fmt; cmd_lint; cmd_test; cmd_docs ;;
    --build)      cmd_build ;;
    --soc)        cmd_soc ;;
    --bootloader) cmd_bootloader ;;
    --sim)        cmd_sim ;;
    --console)    cmd_console ;;
    --sim-b0)     cmd_sim_b0 ;;
    --all)        cmd_fmt; cmd_lint; cmd_test; cmd_docs; cmd_soc; cmd_build; cmd_bootloader; cmd_sim; cmd_console; cmd_sim_b0 ;;
    --help|-h)    usage; exit 0 ;;
    *)            echo "unknown command: $arg" >&2; usage; exit 2 ;;
  esac
done
