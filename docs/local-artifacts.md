# Local development artifacts

Maintained controller mapping probes and their portable tests live in
`tests/controller-mapping-m0/`. CI runs them from that directory. The ESP32
stack-usage diagnostic lives at `scripts/companion-stack-usage.py`; it consumes
the board build's `compile_commands.json` and writes generated reports under
`.research/target-stack/`. The Windows WinMM vJoy reader lives at
`tests/manual/read-vjoy.py`; it reads joystick ID 3 without acquiring a feeder
lease and accepts `--seconds` and `--output`.

## Source and context

Required source, configuration and maintained tools belong in their application
directories and Git. Decisions and verification summaries belong in `docs/`.
Neither the workspace `.research/` nor Cockpit's `.research/` is required to build
or run the app. Do not preserve obsolete material by creating cleanup archives.
Workspace dependency links under `node_modules/@opensimgear/` must resolve to
the current checkout's tracked `packages/`, never to a research source snapshot.

The 2026-11-20 cleanup removed generated builds, coverage, logs, tool downloads,
issue drafts, patches and frozen source snapshots from both research folders.
The remaining local admission, helper-focus and hardware-acceptance notes were
already covered by the [native capture contract](reactive/native-capture-evidence.md),
[native admission verification](verification/2026-11-03-native-port-admission.md),
[desktop output contract](desktop-output-contract.md),
[helper focus verification](verification/2026-11-05-desktop-helper-focus.md), and
[remaining acceptance ledger](reactive/remaining-acceptance.md).
Historical evidence paths in verification notes may no longer exist; deleting
generated evidence does not rerun or extend the recorded qualification.

## Runtime data

The local development instance uses `.tmp/dev-runtime/` for its database,
companion data, ownership locks and `logs/`. This is generated, ignored state,
not source or an archive. Stop every process using the namespace before moving
or deleting it. Select this namespace with `OSG_DCS_BIOS_CONFIG` pointing to
`.tmp/dev-runtime/dcs-bios.json` and `OSG_SQLITE_DATABASE` pointing to
`.tmp/dev-runtime/cockpit.sqlite3`; use absolute paths when launching processes.
Ordinary application launches otherwise use the platform configuration root.
See [SQLite persistence](sqlite-persistence.md) for seeding and reset behavior.

## Rust build outputs

`src-tauri/target/` is disposable Cargo output. `debug/deps/` contains compiled
dependencies and test binaries, while `debug/incremental/` holds incremental
compiler caches. Debug symbols and separate dependency, feature, compiler-flag
and coverage configurations can accumulate large outputs. These files belong
outside Git and contain no authoritative app configuration.

For a complete reset, stop the app and Rust build/test processes, then run
`cargo clean --manifest-path src-tauri/Cargo.toml` from Cockpit. The next native
build must recompile its dependencies. Do not remove a running executable or
its required runtime resources during cleanup.
