# Building our own second-stage bootloader

**Status:** design + staged plan · 2026-09-21
Answers "can we own the bootloader so boot behaviour is ours to change?" — yes, with a specific
contract, a size budget and a staged path that proves each step in esp32sim first.

The current v1 decision (ADR-009) is to **buy** the ESP-IDF second stage. This document is the
alternative track: what full ownership costs, what it buys, and how to get there without betting the
product on a boot-loop.

---

## 1. The contract the ROM imposes

The mask ROM is not configurable. On ESP32-S3 it:

1. reads the image header at flash `0x0` (magic `0xE9`, segment count, entry point, checksum);
2. loads the segments to their load addresses (IRAM/DRAM) and jumps to the entry point;
3. provides the UART/USB download mode that can always reflash a bricked board.

So "our bootloader" is an **ESP image at 0x0** whose code runs from IRAM, before the flash cache and
MMU exist. Everything else — flash init, partition table, image verification, cache/MMU mapping, app
jump — is our job. There is no `main`, no heap, no `esp-hal`: `esp-hal` assumes the app environment.

## 2. What it must do (minimum viable stage 2)

| Step | Why | Difficulty |
|---|---|---|
| UART init + banner | visibility when everything else fails | low |
| SPI NOR init/read | the ROM leaves flash usable, but we must read the partition table and app | medium |
| Partition table parse + validate | reuse `esp-sdk-boot` (already host-tested) | low |
| otadata select + rollback state | reuse `esp-sdk-boot` | low |
| App image parse (header, segments, checksum, SHA-256) | decide whether to boot | medium |
| Cache + MMU mapping of IROM/DROM segments | the app executes from flash at fixed virtual addresses | **high** |
| Jump to entry with the right ABI/stack | hand-off | low |
| Watchdog + reset loop protection | bad image must retry/rollback | low |

The MMU/cache step is the real work: S3 maps flash in 64 KiB pages, the app links for fixed
addresses (`0x4200_0000` IROM, `0x3C00_0000` DROM), and getting the page table, cache mode and
permissions wrong produces a silent hang with no console.

## 3. Budget and constraints

| Constraint | Value |
|---|---|
| Load address | `0x0`; partition table at `0x8000` |
| Size budget | ≤ 32 KiB unless `PARTITION_TABLE_OFFSET` is moved (bootloader and app must agree) |
| Runtime memory | IRAM code, DRAM data, a small stack; no heap |
| Toolchain | `xtensa-esp32s3-none-elf`, `-T` linker script placing `.text` in IRAM |
| Debug | UART only; no backtrace until we write one |
| Recovery | ROM download mode (GPIO0 + reset / USB) always works with secure boot off |

## 4. What full ownership buys

- **Boot menu / recovery mode** on UART, before the app: hold a key, boot slot B, enter USB recovery.
- **Custom A/B policy**: N failures, boot-attempt counters, "boot only after self-test", fast boot
  paths, boot reason reporting.
- **Our own image format** (if we ever leave ESP-IDF compatibility): compression, delta slots, tags.
- **Signature policy**: Ed25519 verification in the bootloader instead of only in the OTA client.
- **Branding**: the banner is ours; no "ESP-IDF" line.
- **Deterministic timing**: skip IDF's optional init we do not need.

## 5. What it costs

- **Secure boot v2**: ROM verifies the *stage 2* signature against eFuse keys. A custom bootloader is
  compatible only if we sign it with the same scheme (`espsecure.py sign_data`), and any mistake
  bricks until the eFuse is burned correctly. Doable, but not on day one.
- **Flash encryption**: the bootloader is responsible for decrypting the app in place. Reimplementing
  this is a research project; until then, encrypted flash is off.
- **Anti-rollback (eFuse)**: same category — off.
- **Every flash/PSRAM variant** must be supported by our init code; IDF's bootloader already handles
  them.
- **Time**: 3–5 weeks for a production-quality stage 2 with OTA/rollback; debugging is UART-only.

## 6. Staged plan (each gate tested in esp32sim)

| Stage | Deliverable | Gate |
|---|---|---|
| **B0 ✅** | Image at 0x0 that prints a banner from IRAM and halts | boots in esp32sim; UART output visible |
| **B1** | Partition table + otadata read; pick a slot; print the decision | matches `esp-sdk-boot` unit-test expectations |
| **B2** | MMU/cache mapping + jump to the existing app | app banner appears after our bootloader banner |
| **B3** | Image checksum + SHA-256 verification; refuse bad images | corrupted image → recovery/rollback |
| **B4** | Rollback state machine + boot-attempt counter | bad app returns to previous slot |
| **B5** | Boot menu / recovery entry; Ed25519 signature check | signed images only; menu usable on UART |

B0–B2 prove the hard parts. B3–B5 are features on top and can land incrementally.

### B0 result (landed)

`bootloader/` is a 100-byte ESP image at 0x0. The ROM log shows:

```
load:0x403c0000,len:0x64
entry 0x403c0008

Sprchuoi ESP32 Boot
[esp-sdk] B0 stage-2, running from IRAM
```

`esp_sdk.sh --sim-b0` builds it, converts the ELF with `esptool elf2image`, boots esp32sim and
asserts the load address and the product banner (`Sprchuoi ESP32 Boot`). Three things learned the hard way:

1. Xtensa inline assembly requires `#![feature(asm_experimental_arch)]` on the esp toolchain.
2. `l32r` literals must be emitted **before** the referencing code; otherwise the linker fails with
   "literal placed after use". The linker script now orders `.literal` first.
3. The ROM leaves a valid stack pointer, but B0 makes no calls, so window spills cannot occur yet.
   B1 needs a runtime (window vectors) before it can call Rust.

The existing `boot/` crate is reused wholesale for the table/otadata/descriptor logic; only the
platform pieces (flash read, MMU, jump, console) are new and unsafe.

## 7. Layout

```
bootloader/
├── Cargo.toml            # no workspace lints: this crate needs unsafe
├── linker.ld             # IRAM text, DRAM data, small stack
├── build.rs              # link args, image header
└── src/
    ├── main.rs           # _start, banner, boot flow
    ├── console.rs        # UART0 MMIO
    ├── flash.rs          # SPI NOR read (ROM-assisted)
    ├── image.rs          # ESP image parse + checksum (delegates policy to boot/)
    ├── mmu.rs            # cache + MMU mapping (the hard part)
    └── jump.rs           # hand-off to the app
```

`bootloader/` is excluded from the host workspace like `examples/`; it only builds for Xtensa.

## 8. The cheaper alternative: a boot-manager app

If the goal is *custom boot features* rather than *owning stage 2*, a smaller design gets most of it:

- keep the IDF bootloader (buy), enable app rollback;
- put a small **boot-manager/recovery app** in the factory slot;
- it presents the UART menu, owns OTA, writes `otadata`, and reboots into the chosen slot;
- a failed app rolls back to the boot manager, which can repair or reflash.

Cost: days, not weeks. Limits: the menu runs after ~100 ms of bootloader, and stage-2 behaviour
(banner, secure boot policy, pre-app USB recovery) is still IDF's.

## 9. Decision

| Option | Effort | Control | Risk |
|---|---|---|---|
| A. Keep IDF stage 2 + boot-manager app | 3–5 days | boot features after stage 2 | low |
| B. Own stage 2, stages B0–B5 | 3–5 weeks | everything before the app | medium; recoverable via ROM |

**Recommendation:** start with A if the immediate need is a boot menu / recovery / OTA control.
Start B now only if the product needs pre-app recovery, custom signature policy, or branding — and
even then, land B0–B2 as a spike behind a flag while the IDF bootloader remains the shipping default.

Either way the app image stays ESP-IDF-compatible, so switching later does not touch the app.
