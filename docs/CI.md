# CI and testing

**Status:** draft v0.1 · 2026-09-21
What runs on every push, what runs on hardware, and what a green build actually proves.

---

## 1. Principles

- **Fast feedback first.** Formatting, clippy, host tests and rustdoc run in parallel and finish in
  under two minutes. Device builds are slower and gated until there is something to build.
- **CI grows with the tree.** A job activates itself when its target appears (`soc/*`, `tools/gen`,
  HIL runner); it never turns red because a future phase has not landed.
- **The device build is not optional at release.** Phase 2 adds `soc/esp32s3`; from then on the S3
  build is a required check, and Phase 8 adds C6.
- **Tests are host-first.** Scheduler, codec, config generation, contract fixtures and driver logic
  with mocks run on `x86_64`. Hardware is the final gate, not the first one.
- **No flaky gates.** A test that needs a board lives in the HIL job, not in the PR job.

---

## 2. Jobs

| Job | Runs | What it proves |
|---|---|---|
| `fmt` | every PR | `cargo fmt --all -- --check` |
| `lint` | every PR | `cargo clippy --workspace --all-targets -- -D warnings` |
| `test` | every PR | `./esp_sdk.sh --test` (163 tests: HAL mocks, kernel, drivers, net, boot), then `cargo doc` with `RUSTDOCFLAGS=-D warnings` |
| `msrv` | every PR | workspace compiles on Rust 1.95 (the `esp-hal` MSRV) |
| `nostd` | every PR | workspace builds for `x86_64-unknown-none`, proving no accidental `std` dependency |
| `device-esp32s3` | when `soc/esp32s3/` exists | release build for `xtensa-esp32s3-none-elf` with the esp toolchain |
| `device-esp32c6` | when `soc/esp32c6/` exists | release build for `riscv32imac-unknown-none-elf` on upstream stable |
| `config-matrix` | when `tools/gen/` exists | every defconfig + `allyesconfig` + `allnoconfig` + strict board binding |
| `hil` | when `HIL_RUNNER` is set | flash, boot, provisioning, MQTT contract, OTA rollback on a real board |
| `simulation` | when `SIMULATION` is set | builds the toolchain + esp32sim, boots `hello-kernel` (asserted) and the B0 bootloader image (asserted) |

Gating is done with a `gate` step after checkout (job-level `hashFiles` cannot see a workspace that
has not been checked out yet). A skipped device job prints a notice, so "green" is never confused
with "verified".

---

## 3. Local equivalents

```bash
./esp_sdk.sh --check        # fmt + clippy + tests + docs
./esp_sdk.sh --nostd        # bare-metal target build
./esp_sdk.sh --build --sim  # device image + emulator assertion
./esp_sdk.sh --sim-b0       # custom bootloader in the emulator
cargo +1.95.0 check --workspace --all-targets     # MSRV, once the toolchain is installed
```

`cargo xtask ci` will wrap these plus the device matrix once `xtask` exists (Phase 2); until then the
commands above are the contract, and CI runs exactly them.

---

## 4. Caching

| Cache | Key | Effect |
|---|---|---|
| `Swatinem/rust-cache` | `Cargo.lock` + toolchain | incremental target dir, ~30 s saved per job |
| xtensa toolchain (future) | `espup` version + action version | avoids re-downloading ~1 GB on every run |
| `sccache` (future) | per-target | shared compilation across matrix jobs |

Cache misses must never fail a job.

---

## 5. Test layers

| Layer | Where | Examples |
|---|---|---|
| Unit | host, `cargo test` | scheduler queues/timers, config symbol mapping, topic encode/decode |
| Snapshot | host | `.config` + `board.dts` → generated files compared byte-for-byte |
| Contract | host (fixtures from hardware) | `smart_home_idf` MQTT payloads, discovery JSON |
| Integration | host with mocks | probe order, deferred probe, KV power-cut simulation |
| On-target | HIL | ISR latency, DMA stress, EEPROM write cycle, sleep current |
| Emulator | esp32sim | boot chain, scheduler, IPC, diagnostics (`esp_sdk.sh --sim`) |
| Soak | nightly / pre-release | 72 h AP flap + broker restart, 7-day fault injection |

Coverage is tracked for host-tested crates (target ≥ 80 % on kernel/config/contract); the number is
reported, not used as a gate until the crates stabilize.

---

## 6. Release gate (Phase 6+)

On a `v*` tag:

1. `cargo semver-checks` against the previous tag.
2. `cargo package --list` for every crate; `cargo publish --dry-run` in dependency order.
3. Device builds for every Tier-1/Tier-2 target.
4. Size budget check per defconfig (flash/RAM diff vs the last release).
5. Changelog generated, tag signed, crates published, docs.rs rebuild watched.

The release workflow is added with Phase 6; until then releases are manual and follow this list.

---

## 7. Policies

- **Branch protection:** `fmt`, `lint`, `test`, `msrv` required; `device-*` required once present.
- **Warnings are errors** in CI (`-D warnings` for rustc, clippy and rustdoc). Locally they are
  warnings so development stays smooth.
- **No secrets in workflows.** Publishing uses crates.io tokens in a protected environment.
- **Flaky test policy:** quarantine with an issue link and a 7-day deadline; never `--retry`.
- **`unsafe` audit:** `cargo geiger` (future) reports once `soc/` contains unsafe.

---

## 8. Current state

The repository has the Phase 2 seed: `hal/` (traits + host mocks), `kernel/` (scheduler, time,
`Signal`, IPC, initcalls, service manager, crypto primitives, power policy, diagnostics), `drivers/`
(binding model + AT24 EEPROM), `net/` (link/lease service) and `boot/` (partitions, otadata,
selection, descriptor) with 163 host tests. `third_party/esp32sim` is vendored for emulator boot
tests; the job stays dormant until a firmware image exists. The CI runs fmt, clippy, tests+docs, MSRV and a bare-metal
`no_std` build; device, config-matrix and HIL jobs are present and dormant by design, so the pipeline
does not have to be rewritten when each phase lands.
