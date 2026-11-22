#!/usr/bin/env python3
"""Generate an ESP-IDF partition table binary for the simulation image.

Layout matches what esp32sim writes: bootloader at 0x0, table at 0x8000, app at
0x10000. The factory slot at 0x10000 is therefore where `--app` lands.

Format: 32-byte entries, little endian, followed by an MD5 entry (magic 0xEBEB)
whose digest covers the entries before it.
"""
import hashlib
import struct
import sys

MAGIC = 0x50AA
MD5_MAGIC = 0xEBEB

# label, type (0=app, 1=data), subtype, offset, size, flags
ENTRIES = [
    ("nvs", 1, 2, 0x9000, 0x6000, 0),
    ("phy_init", 1, 1, 0xF000, 0x1000, 0),
    ("factory", 0, 0, 0x10000, 0x300000, 0),
]


def main() -> int:
    table = bytearray()
    for label, part_type, subtype, offset, size, flags in ENTRIES:
        table += struct.pack(
            "<HBBII16sI",
            MAGIC,
            part_type,
            subtype,
            offset,
            size,
            label.encode().ljust(16, b"\0"),
            flags,
        )
    digest = hashlib.md5(table).digest()
    table += struct.pack("<H", MD5_MAGIC) + b"\0" * 14 + digest
    table += b"\xff" * (0xC00 - len(table))
    sys.stdout.buffer.write(table)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
