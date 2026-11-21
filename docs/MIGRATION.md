# Migrating `smart_home_idf` to the SDK

**Status:** draft v0.1 · 2026-09-21
Target: the existing ESP-IDF/C++ firmware in
[`sprchuoi/smart_home_idf`](https://github.com/sprchuoi/smart_home_idf) is replaced by an app built
on `esp-sdk`, on the same ESP32-S3 hardware, with the same external contract.

The SDK is done when *someone else's product* runs on it. `smart_home_idf` is that product.

---

## 1. What must not change

The broker contract, because `Smart_Server` (FastAPI + MQTT bridge) parses it positionally
(`parts[2]`, `parts[3]`, `parts[4:]`). The **code** is authoritative here — the repo's own
documentation contradicts it in several places, which is exactly why the contract gets captured as
golden fixtures before anything is rewritten.

| Item | Current value (from code) |
|---|---|
| Topic prefix | `smart_home/devices/<device_id>` |
| Status | `<prefix>/status`, QoS 1, retained, LWT `{"status":"offline"}` |
| Status document | `{"status":"online","device_type":"sensor_node","name","firmware_version","ip","room","uptime_s","heap","rssi"}` |
| Telemetry | `<prefix>/sensor/<channel>`, QoS 0, `{"value":<f>,"unit":"<u>"}` |
| Command | `<prefix>/command`, QoS 1, subscribed, body `{"command":"<verb>"}` |
| Response / ack | `<prefix>/response`, QoS 1, `{"command":"<verb>","status":"ok",…}` |
| OTA progress | `<prefix>/response`, `{"command":"ota","status":"progress","percent":<n>}` |
| Client id | `<device_id>` (`[a-z0-9_-]`, derived from MAC, persisted) |
| Session | persistent (`clean_session = false`), keepalive 30 s |
| Reconnect | owned loop, exponential backoff + jitter |
| Discovery | **not published** by design (Smart_Server has no Home Assistant) |
| Credentials | NVS: WiFi + MQTT host/port/auth/device id/name/room/OTA URL |
| Console | `wifi_set`, `mqtt_set`, `mqtt_auth`, `mqtt_device`, `mqtt_status`, `mqtt_clear`, `mqtt_ota_url`, plus system commands |
| OTA | `esp_https_ota` incremental API, URL from NVS, **no rollback** |
| Transport | plain TCP MQTT on the LAN, no TLS |

---

## 2. Parity matrix

| Capability | IDF/C++ today | SDK equivalent | Gate |
|---|---|---|---|
| Boot → WiFi → MQTT | FreeRTOS tasks, MQTT on core 0 | kernel classes, `Net` pinned to core 0 | same boot sequence in logs, join time within 20 % |
| WiFi provisioning | `wifi_set <ssid> <password>` → NVS | console `wifi_set` compat alias + SoftAP portal `POST /wifisave` | identical serial transcript |
| MQTT config | `mqtt_set`, `mqtt_auth`, `mqtt_device`, `mqtt_clear`, `mqtt_status` → NVS | KV + same command names behind `compat` feature | identical transcripts |
| Device id | from MAC, `[a-z0-9_-]`, persisted | same derivation, same validation | same id string |
| LWT / online status | status topic, QoS 1, retained | same topic, same payload, same QoS/retain | broker capture diff = empty |
| Telemetry | `sensor/<chan>` QoS 0 | `Sensor` entities map to the same channels | payload diff = empty |
| Commands | `command` JSON verb, ack on `response` | same verbs, same ack shape | reboot/ota round-trip |
| OTA | URL from NVS, no rollback | signed image, `otadata`, rollback | update + rollback on hardware |
| Discovery | none | retained `homeassistant/...` (new) | entities appear in HA — accepted behavior change |
| Logs | `ESP_LOGx` | `defmt` + flash ring | crash survives reset |
| Config store | NVS | KV partition | re-provision (see §4) |

**Deliberate differences** (must be signed off, not discovered):

1. Discovery is published. The C++ build suppressed it because Smart_Server is not Home Assistant;
   the SDK publishes it for the HA path. On a broker where nothing consumes discovery, it is retained
   noise — so it is behind `CONFIG_SUBSYS_SMARTHOME_DISCOVERY` and off in the migration defconfig
   until the HA phase.
2. Rollback exists. A bad image returns to the previous slot instead of persisting.
3. MQTT keepalive/buffer sizes are configurable instead of compile-time constants.

---

## 3. Migration stages

### Stage 1 — Freeze the contract (2–3 days)

- Capture from a running C++ node: `mosquitto_sub -v -t 'smart_home/devices/#'` for boot, telemetry,
  command, reboot, OTA.
- Store as fixtures in `tests/contract/<scenario>.jsonl` with an allow-list for volatile fields
  (`ip`, `uptime_s`, `rssi`, `heap`, `firmware_version`).
- Write the schema down in `docs/contract.md`; note the repo's doc/code contradictions resolved.

### Stage 2 — SDK node with a shadow id (1 week)

- Build `apps/sensor-node` for the devkit with id `<real-id>-sdk`, same broker.
- Replay the fixtures through a differential test: normalise volatile fields, assert byte equality
  on status, telemetry, command acks, LWT.
- Fail the build on any contract drift — this is the migration's regression net.

### Stage 3 — Bench cutover, one node (1 week)

- Flash the SDK firmware on the real node (same `<id>`), run the bench suite: provisioning, HA
  entities (if enabled), reboot command, OTA, 24 h soak.
- Keep the IDF binary as the rollback image in the inactive slot (both are ESP-IDF images, so the
  bootloader can start either).

### Stage 4 — Fleet cutover and decommission (1 week)

- OTA the remaining nodes; keep the IDF build branch alive for one release.
- Remove `smart_home_idf` from the deployment docs; point them at the SDK app.

---

## 4. Credentials and NVS

NVS and the SDK's KV do not share a format. Two options:

| Option | Cost | Risk |
|---|---|---|
| **Re-provision** (chosen for v1) | one console session per node | manual step in the field |
| One-shot importer: read NVS partition at first boot, copy keys into KV | ~3 days + NVS format knowledge | parsing a foreign, versioned format |

The importer is a documented backlog item, not a v1 blocker: the fleet is small and physical access
exists. The console commands keep their names, so the documented provisioning procedure is unchanged.

---

## 5. OTA across the migration

- Both firmwares produce ESP-IDF application images, so the same partition layout and the same Pi
  OTA server serve both. `otadata` hands over cleanly.
- The SDK signs images (SHA-256 + Ed25519). The C++ images are unsigned, so during migration the
  image policy is `allow-unsigned`, then flips to `require-signature` once every node is on the SDK.
  This is a config switch, not a code change, and it is in the migration checklist.
- Rollback: a failed SDK image returns to the slot holding the C++ image, so the worst case is
  "the node is back on the old firmware", not "bricked".

---

## 6. Verification gates

| Gate | Method | Pass |
|---|---|---|
| Contract | differential fixtures vs captured C++ traffic | zero unexplained diffs |
| Provisioning | serial transcript replay | identical commands and responses |
| HA | discovery payload snapshots + real HA | expected entities, correct state/units |
| OTA | bad signature, corrupted image, power-cut mid-write | rollback in all three |
| Stability | 72 h soak, AP flap, broker restart | no missed telemetry > 30 s |
| Power | bench measurement vs C++ baseline | no worse than +10 % average current |
| Rollback | OTA back to the C++ slot | boots and reconnects |

The bench harness already exists (Pi testbench: serial, WiFi AP, Mosquitto, operator prompts), so
these gates are scripts, not manual rituals.

---

## 7. Risks specific to migration

| Risk | Mitigation |
|---|---|
| Documentation does not match code (topic prefix, discovery, heartbeat) | Contract captured from code + golden fixtures; `docs/contract.md` is authoritative |
| NVS data lost on cutover | Re-provision procedure documented; importer backlogged |
| Discovery turns into retained noise on a Smart_Server-only broker | Feature-gated; off in the migration defconfig |
| Behavior drift in timing (keepalive 30 s, 10 s MQTT timeout) | Same defaults in the SDK config; asserted in fixtures |
| Two firmwares, one broker | Distinct device ids during shadow run; never the same id on two boards |
| Feature gap discovered late (e.g. a console command, a status field) | Parity matrix reviewed before Stage 2; additions are cheap, omissions are not |

---

## 8. Exit criteria (migration done)

- One real node runs SDK firmware with the production `<id>` and full HA functionality.
- Contract fixtures pass on the SDK build; no unexplained diff.
- Provisioning and MQTT configuration work with the existing console procedure.
- OTA updates it, and a bad image rolls back — including back to the IDF build.
- 7-day soak with AP flaps and broker restarts.
- `smart_home_idf` is archived with a pointer to the SDK app and this document.
