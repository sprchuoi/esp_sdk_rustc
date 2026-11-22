# B0 — our own second-stage bootloader

Stage B0 of the gated bootloader track ([docs/BOOTLOADER.md](../docs/BOOTLOADER.md)). The mask ROM
loads this image from flash 0x0, copies its segment to IRAM at `0x403c0000`, and jumps to `_start`.
It prints a banner over UART0 (already configured by the ROM) and halts.

This is deliberately assembly-only: no stack use, no windowed-ABI calls, no runtime. It proves the
ROM contract before we add flash reads, partition tables and MMU mapping.

## Build and run

```bash
../scripts/bootloader-b0.sh      # builds, makes the ESP image, boots it in esp32sim
```

Expected:

```
load:0x403c0000,len:0x14c
entry 0x403c0004

[esp-sdk] bootloader B0: alive, running from IRAM
```

## Constraints learned here

- The image must be an ESP image at 0x0; `esptool elf2image` encodes segments and entry point from
  the ELF.
- Xtensa inline assembly needs `#![feature(asm_experimental_arch)]` on the esp toolchain.
- `l32r` literals must be placed **before** the code that uses them, or the linker rejects the
  relocation ("literal placed after use").
- The ROM leaves a valid stack pointer, but B0 never calls Rust, so window spills cannot happen yet.
  B1 will need a runtime (vectors or hand-written window handlers) once it calls functions.
- The binary is ~400 bytes; the 32 KiB budget is not a concern yet.

## Next (B1)

Read the partition table and `otadata` through SPI NOR and print the decision, reusing `boot/`'s
host-tested parsing. That requires a callable runtime and a small stack.
