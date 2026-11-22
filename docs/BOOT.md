# Boot and OTA contract

**Status:** draft v0.1 · 2026-09-21 · implemented in `boot/`
How the application behaves around an ESP-IDF bootloader it does not own (ADR-009), and why that
is still a designed contract rather than a dependency.

---

## 1. Division of labour

| Stage | Owner | Responsibility |
|---|---|---|
| ROM | silicon | load the 2nd stage from 0x0, UART/USB download mode (unbrick) |
| 2nd-stage bootloader | ESP-IDF binary (~21 KB at 0x0) | flash/PSRAM init, partition table, otadata check, image validation/secure boot, jump |
| App (entry `_start`) | us | read reset reason, verify its own state, confirm or reject the image, run |

We deliberately do not reimplement the bootloader. What we own is everything the app must get right:

- the **partition table** (`boot::partition`),
- the **otadata** entries that tell the bootloader which slot to start (`boot::otadata`),
- the **selection rules** mirrored app-side for diagnostics and tests (`boot::select`),
- the **app descriptor** the OTA client reports (`boot::appdesc`).

All of it is pure logic over byte slices, so it is host-tested against the same rules ESP-IDF
implements — and against CRC vectors produced by `otatool.py`.

---

## 1.1 Why the console says "ESP-IDF v5.5.1 2nd stage bootloader"

That line comes from the **bootloader binary**, not from the application. ADR-009 buys the second
stage from ESP-IDF (Apache-2.0) instead of writing it, so its compiled-in banner is what you see.
It does **not** mean the app runs ESP-IDF or FreeRTOS.

The analogy is a PC: UEFI/GRUB boots first, then Linux runs. Here the ROM loads the IDF bootloader,
which initialises flash/PSRAM, reads the partition table, validates the image and jumps. Everything
after `Loaded app from partition at offset 0x10000` is our `no_std` image: TinyRTOS scheduler, IPC,
diagnostics. No FreeRTOS, no `app_main`, no ESP-IDF task code is linked into the app.

What that specific binary is:

| Property | Value |
|---|---|
| File | `boards/esp32s3-devkitc-1/bootloader.bin` |
| Built from | ESP-IDF v5.5.1, `esp32s3` |
| Secure boot / flash encryption | off |
| App rollback | **disabled** (Phase 0 replaces it with a rollback-enabled build) |
| Size | 21 056 bytes at 0x0 |

If we later build our own second stage (Phase 9 spike), the banner becomes ours — but so does the
work: flash/PSRAM init, partition parsing, image checksum/SHA-256, MMU mapping, `otadata` handling,
and we would lose secure boot and encryption, which the IDF bootloader implements for free.

## 2. Flash layout (16 MB, N16R8)

| Region | Offset | Size | Notes |
|---|---|---|---|
| bootloader | 0x0 | ≤ 32 KiB | IDF 2nd stage; rollback-enabled build ships from `xtask` |
| partition table | 0x8000 | 4 KiB | generated from the board description |
| nvs | 0x9000 | 24 KiB | bootloader/phy calibration data |
| otadata | 0xf000 | 8 KiB | two 4 KiB sectors, one entry each |
| phy_init | 0x11000 | 4 KiB | RF calibration |
| ota_0 | 0x20000 | 4 MiB | slot A |
| ota_1 | 0x420000 | 4 MiB | slot B |
| kv | 0x820000 | 128 KiB | config store |
| log | 0x840000 | 64 KiB | forensic ring |

`boot::partition::validate()` rejects a generated table when a partition is misaligned (4 KiB),
overlaps another, runs past the flash size, has a duplicate/empty label, or when there is no app
partition. Those are build-time failures, not field failures.

---

## 3. otadata

Two sectors of 4 KiB; only the first 32 bytes of each are meaningful. Layout is bit-compatible with
`esp_ota_select_entry_t`:

| Offset | Field | Meaning |
|---|---|---|
| 0 | `ota_seq` (LE) | 1-based slot counter; active slot = `seq - 1` |
| 4 | `seq_label[20]` | legacy, ignored |
| 24 | `ota_state` (LE) | 0 new, 1 pending verify, 2 valid, 3 invalid, 4 aborted, 0xFFFFFFFF undefined |
| 28 | `crc` (LE) | CRC-32 over the four `ota_seq` bytes |

The CRC uses ESP-IDF's ROM convention — `esp_rom_crc32_le(UINT32_MAX, &seq, 4)`, which is *not* the
plain zlib CRC of the same bytes. `boot::crc32` implements that convention and is pinned by vectors
computed the same way `otatool.py` does:

| `seq` | CRC |
|---|---|
| 0 | `0xFFFFFFFF` |
| 1 | `0x4743989A` |
| 2 | `0x55F63774` |
| 3 | `0xED4A5011` |

An entry is **valid** when the CRC matches, `seq != 0xFFFFFFFF`, and the state is neither `INVALID`
nor `ABORTED` — the same predicate as `bootloader_common_ota_select_valid()`. Among valid entries
the higher `seq` wins; a tie prefers index 0 (IDF's `MAX()` rule).

---

## 4. Selection and fallback

`boot::select::decide()`:

```
valid otadata?
  → yes: slot = seq-1, find partition subtype "ota_<slot>"
         found  → Slot { index, partition }       reason: OtaSelected
         missing→ fallback
  → no:  factory partition? → Factory            reason: FactoryFallback
         else first app?    → Slot { 0 }          reason: FirstAppFallback
         else               → Halted               reason: NoApp
```

The app mirrors this rule for diagnostics ("why did we boot slot 1?") and to decide whether to
confirm itself. The real decision is still made by the bootloader; a disagreement is a bug the tests
catch, not something we paper over.

---

## 5. Rollback flow

1. OTA worker streams the image into the inactive slot, verifies SHA-256 + Ed25519 (Phase 5), then
   writes `otadata` with `seq = active + 1`, `state = New`.
2. Reset. Bootloader sees the higher valid `seq`, marks the image `PendingVerify`, boots it.
3. The app starts, runs self-checks (network, storage, sensors) with a deadline (e.g. 60 s).
4. On success it writes `state = Valid` and commits. On failure or timeout it *does not* confirm.
5. The next boot sees `PendingVerify` and the bootloader marks it `Aborted` and returns to the
   previous slot.
6. `Invalid` is for an image the app actively rejects (e.g. self-test failure immediately after
   boot); it is never selected again.

The important asymmetry: **the new image must confirm itself, not the old one**. A power cut during
the confirmation window is indistinguishable from a failure, which is why the window must be short
and the confirmation write must happen as soon as the checks pass.

---

## 6. App descriptor

`boot::appdesc` parses `esp_app_desc_t` at image offset `0x20`: magic `0xABCD5432`, then version,
project name, build time/date and IDF version as NUL-padded strings. It is what `ota` reports as the
running version and what the migration uses to distinguish SDK builds from the C++ build on the same
broker.

---

## 7. What the app does *not* do

- It does not write `otadata` on every boot; the state field only changes on OTA confirmation.
- It does not verify the bootloader or secure boot (not enabled; Phase 6+ decision).
- It does not reimplement image validation; the IDF bootloader checks the image header, checksum and
  (if configured) signature. Our SHA-256 + Ed25519 check is an *additional* app-level gate for the
  image we are about to stage.
- It does not assume the factory partition exists — some tables only have `ota_0`/`ota_1`.

---

## 8. Verification

| Check | Where |
|---|---|
| otadata layout, CRC, state rules, selection | `boot/src/otadata.rs` tests |
| partition alignment/overlap/bounds/no-app | `boot/src/partition.rs` tests |
| decision matrix (selected, factory, first app, halt) | `boot/src/select.rs` tests |
| descriptor parsing, bad magic/short/non-UTF-8 | `boot/src/appdesc.rs` tests |
| IDF compatibility | CRC vectors derived from `otatool.py`; layout from `esp_flash_partitions.h` |
| On-target | OTA a good image, OTA a bad image, power-cut during confirmation (Phase 5/7) |

The remaining gap before rollback is trustworthy on hardware is the **rollback-enabled bootloader**
(Phase 0 ground truth): a prebuilt bootloader with `CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE` off will
ignore `PendingVerify` and boot the new image anyway. The app-side state machine is still correct;
it just has no teeth until that bootloader is in place.

---

## 9. Open items

1. SHA-256 image digest + Ed25519 signature verification (Phase 5, with `crypto`).
2. Anti-rollback counter via eFuse (explicitly out of scope until after v1.0).
3. Reading the reset reason and crash record from RTC RAM on the first boot after an update.
4. A factory-recovery image policy (keep a known-good image in the `factory` slot, or rebuild it
   from `ota_0` on first boot).
