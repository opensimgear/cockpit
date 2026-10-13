# Repository-owned end-to-end tests

Run from the repository root with Python 3.10+, Git and a C11 compiler:

```sh
python3 tests/e2e/run.py --sanitize
```

Cockpit tests also require Node.js, pnpm and the Rust/Tauri platform prerequisites.
Close other Cockpit desktop/browser sessions and stop their development watchers
for the run. A gateway has one CDC client; another Cockpit instance can automatically
connect to a test gateway while its layout is being applied or its test client reconnects.

The initiating repository owns the runner. It clones remote repositories listed in
`dependencies.json` into its ignored `.e2e/deps/`, initializes submodules recursively,
checks that both firmware repositories declare the same firmware-common commit,
and builds each runtime through that repository’s `simulation/build.py`.
The exact common pin is cloned automatically. No parent setup or adjacent checkout
is consulted. Checkouts, builds, state and reports all stay under `.e2e/`.

`--prepare-only` provisions components without starting processes. The resulting
`.e2e/launch.json` names the binaries and owning source trees; `.e2e/resolved-dependencies.json`
records repository commits and any local working-tree snapshot hashes. Test evidence
is stored under `.e2e/runs/`. Remote application refs are intentionally `main` while
the project is work in progress; the resolved manifest records what a run actually used.

For unpublished cross-repository development, pass explicit source overrides:

```sh
python3 tests/e2e/run.py --sanitize \
  --source device-gateway=/absolute/path/to/device-gateway \
  --source device-template=/absolute/path/to/device-template
```

An override still creates an isolated Git clone, applies the source’s tracked diff
and nonignored new files, and initializes its dependencies. It never modifies or
builds inside the source checkout. Clean submodule commit changes are initialized
after the snapshot is applied; uncommitted nested submodule contents are rejected
with a clear error. `.e2e/`, `.simulation/`, build outputs and other
ignored files are not copied. This is how coordinated changes can be tested before
they are published; default remote runs require those changes to exist on the
configured remote refs. A pinned firmware-common override uses the exact declared
commit, without local changes.

Only checkouts with a bootstrap receipt are replaced when sources change. Delete
this repository’s `.e2e/` to force clean provisioning. Run one bootstrap per repository
at a time. Bootstrap retry, executable-mode and submodule behavior can be checked
without network access using `python3 -m unittest discover -s tests/e2e -p 'test_*.py'`.

The first Cockpit scenario starts its normal native gateway service before the gateway,
then launches three JSON-configured devices. It verifies live discovery, full
configuration Apply & save, registration and dynamic layout Apply, real input
reports, neutral offline slots, device/gateway crash recovery, saved configuration,
writer contention after Cockpit restart, and independent process lifetimes.

The simultaneous-gateway scenario connects two gateway processes through one normal
Cockpit service. Each gateway has a device with the same identity, which verifies
that configuration caches, writer sessions, HID layouts and input reports are scoped
to the gateway. It checks aggregate discovery, rejected commands without a gateway
identity, independent errors/disconnects, and crash recovery while the other gateway
keeps its configuration writer and accepts saved changes. Correlated browser requests
also verify that a paused gateway cannot block status requests to its peer.

Native simulations execute the production configuration, flash journal, Cyphal
transport, registry and descriptor code. Loopback TCP carries CDC bytes and UDP
carries CAN frames. These tests validate protocol/report behavior; native streams
do not enumerate OS USB/HID devices. Physical USB/CAN acceptance remains separate.
