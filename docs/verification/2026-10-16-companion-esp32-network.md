# ESP32 companion network checkpoint — 16 October 2026

Issue [#63](https://github.com/opensimgear/cockpit/issues/63) remains in progress. These are local, uncommitted/unpushed working-tree changes on top of the rendering foundation.

## Implemented

- Screen-independent serial Wi-Fi provisioning, a trusted Unix clock supplied over the cable and a helper that hides Wi-Fi/pairing input and prints only bounded status tokens.
- RAM-only Wi-Fi driver and installation credentials by default. Optional persistence is rejected before secret exchange unless NVS encryption is configured and flash encryption is already enabled. Protected resume requires a fresh serial clock. No eFuses/security settings are changed automatically.
- Existing Cockpit WSS endpoint/subprotocol, offered CA SPKI pin and CA validity/constraint checks before secret/token transmission, with ordinary SDK chain/time/hostname/IP verification. No public trust store, bypass, plaintext fallback or redirect.
- Host-approved pairing, assignment selection, explicit reconnect/stop/forget/page commands, immutable published panel delivery and session-bound heartbeat feedback. The client sends no baseline/input and cannot arm touch/game output.
- Ordered and digest-checked chunked transfers, source publication hash verification, target capability/resource checks, duplicate-key/NUL/nonfinite/depth/node rejection and bounded JSON allocations including allocator metadata.
- Two staged image slots and immutable display snapshots. Driver waits do not hold the network-state mutex, and a drawing image cannot be overwritten. Invalid transfers preserve the previous panel visibly disconnected/disarmed.
- Generic screen/board API remains independent of Wi-Fi, JSON and host protocol. Default display is headless; no physical pin/controller assumption was added.

Cockpit expiresAtMs uses the host's monotonic clock. Firmware treats that field as opaque host metadata, relies on host expiry enforcement and applies its own 60-second monotonic approval deadline. It never compares that field with the TLS wall clock.

## Validation

- Native C wire/publication/settings/client tests pass with strict warnings-as-errors, AddressSanitizer and UndefinedBehaviorSanitizer. They cover successful pairing/assignment/live delivery, disconnect/revoke-style rejection, explicit reconnect, invalid session IDs/counters, transfer corruption/order, incompatible publication features, typed feedback/ranges/units, resource bounds, allocation cleanup, protected-storage refusal and serial overflow/cleanup.
- Measured portable network coverage (lines / branches / functions): json 100 / 73.99 / 100%; publication 100 / 74.40 / 100%; settings 100 / 74.17 / 100%; client 97.41 / 75.42 / 94.44%. Each passes 71% gates. Wi-Fi/TLS/NVS platform adapters are cross-compiled, not counted as native covered modules.
- Three Python provisioning encoding/status/size tests pass without opening a serial port.
- Existing renderer native suites and sanitizer/coverage gates pass; source publication export tests pass (96.83% lines, 95.59% branches, 92.86% functions). A drift check ties the offline generated header, live publication fixture and host fixture to the same canonical publication/hash/capability fingerprint.
- The added Rust integration test passes against the actual Cockpit Hub: publish the same embedded fixture, pair/approve, assign/authenticate/open, receive feedback on a heartbeat without baseline/input, then reject after revocation. This is an in-process host protocol test, not real-WSS radio evidence. Windows Cargo incremental compilation was disabled because the WSL share does not support its lock operation.
- Final ESP-IDF v5.5.2 cross-builds pass for ESP32 (819856-byte application) and ESP32-S3 (812944-byte application), each within the configured single 0x177000-byte app partition. Display adapters are headless. No board was flashed.
- Scoped original-file whitespace checks pass. The new CI definition includes native/host tests and both SDK targets; remote CI execution has not been claimed.

Reproduce using [the firmware guide](../../apps/companion-esp32/README.md). Native evidence is in .research/esp32-network and .research/esp32-native; final SDK logs are .research/esp32-network-build.log and .research/esp32s3-network-build.log. cJSON source/notice provenance is recorded under apps/companion-esp32/third_party.

## Remaining qualification and scope

Physical Wi-Fi/serial, endpoint certificate failure behavior on the actual ESP32, protected NVS/flash provisioning and reset/recovery, actual display/controller/touch drivers, MCU peak memory/performance and network/power-failure qualification remain open. RAM-only reboots require re-pairing. Downloaded panels are not persisted across reboot. Image decoding/richer widgets, designer UI target integration, live touch input and its baseline/lease/arming path remain unfinished #63 work. IPv6 discovery/endpoints are not implemented in this client.

No actual LAN, game/backend, physical screen, secure-storage or radio success is inferred from native fixtures or cross-builds. No #63 hardware acceptance checkbox is marked complete; OTA and broader board qualification remain #64/#65.
