# Repository-owned end-to-end tests

## Development status and disclaimer

This project is heavily under development and is not ready for use. Use it entirely at your own risk. To the fullest extent permitted by applicable law, the authors and contributors disclaim all liability for any damage, loss, or other issues arising from its use.

Run from the repository root with Python 3.10+, Git, Node.js, pnpm, a C11 compiler and the Rust/Tauri platform prerequisites:

```sh
python3 tests/e2e/run.py --sanitize
```

Use an isolated test host. Close other Cockpit sessions and development watchers before starting the normal native service: it owns real discovery providers as well as the simulated endpoints. Local checks while Cockpit is running must use pure fixture/oracle tests or isolated production-C simulation without another native service.

The runner clones the exact application refs in `dependencies.json`, initializes their submodules and requires both applications to declare the same firmware-common baseline. The initiating suite's explicit full firmware-common ref selects the shared contract actually built. `resolved-dependencies.json` records each resolved commit and the firmware-declared baseline separately. This permits testing the published atomic capture-at-claim contract against unchanged application sources without silently changing application refs.

Checkouts, builds, state and reports stay in ignored `.e2e/`. `--prepare-only` builds components and writes `launch.json` without starting acceptance processes. `--output-dir /absolute/path/to/.research/cockpit-e2e` selects a separate evidence directory. Only checkouts with a bootstrap receipt are replaced. `--source device-template=/absolute/path` and `--source device-gateway=/absolute/path` snapshot explicit local sources into isolated clones; source repositories are never built or modified. Dirty nested submodules are rejected. A firmware-common source override uses the exact manifest commit without local edits.

Pure provisioning and portable graph tests run without the native service:

```sh
python3 -m unittest discover -s tests/e2e -p 'test_*.py'
node --test tests/e2e/profile-graph-fixtures.test.mjs
```

Each simulation uses a fresh `OSG_SIMULATION_NAMESPACE`, selecting a separate loopback discovery directory. This isolates C peers from other simulation scenarios; it does not make a second native service safe beside an active desktop.

The existing device lifecycle scenario starts the native service before a gateway, then enrolls three independent production-C devices. It checks persistent configuration, explicit layout updates, real input reports, released startup baselines, neutral offline ghosts, disabled online configuration, offline Forget and independent device/gateway restarts. The simultaneous-gateway scenario preserves gateway-scoped caches/writers/layouts, identical module identities on two gateways, peer isolation and responsiveness. The manual action scenario retains descriptor/revision/argument validation, actual readback, unchanged persistent settings, endpoint contention and ambiguous-acknowledgement no-replay assertions.

The unified physical Setter scenario uses the ordinary native Profile service, cached native inventory and exact observed port/contract pins. An explicit SetOnChange node requests an advertised readable RGB Setter. Production-C readback verifies its output, a competing writer is rejected, and Pause/Stop restore the actual prior RGB captured atomically at claim. A fresh Apply gets a new owner. Configuration flash counters, configuration contents and controller layout remain unchanged. Restart retains the portable document without resuming its old output owner. Legacy independent mapping/DCS/integration/game/visibility starts are rejected. Evidence is written as **software simulation**, with hardware and platform qualification flags false.

The Games scenario uses fresh installation metadata with an inert `DCS.exe` fixture that is never executed. It rejects retired game-profile authoring/activation and forged browser launch authority, retains explicit installation metadata across restart, and leaves obsolete game configuration bytes untouched. Actual game process/context launch acceptance remains in the native Manager/Broker tests; the inert file does not prove a running game.

The independent DCS-BIOS radio peer scenario retains named command handling, bounded diagnostics, complete/fragmented/frozen export counters and malformed-frame rejection. It supplies a transport/decoder oracle without reviving a standalone DCS runtime.

The three repository-owned native acceptance scripts exercise saved unified Profile CRUD/compile/Apply/Pause/Stop, immutable applied revisions and old-owner rejection, plus real pinned-CA WSS pairing and isolation. Companion tests use exact native observed port hashes, actual reducer values and local/host feedback provenance, including 64-output chunked transfer and per-peer freshness. Retired standalone authoring/start/feedback namespaces fail without replacing the applied owner. Private feeder startup, unavailable optional backend checks and platform qualification gates remain unchanged.

Native C simulation executes production configuration, flash journals, Cyphal, registry, endpoint actions and descriptors. Loopback TCP carries CDC bytes and UDP carries CAN frames. It does not enumerate OS USB/HID devices, qualify electrical hardware, confirm game delivery, or enable virtual/script/HidHide qualification.
