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

For workspace research directories, add `--output-dir /absolute/path/to/.research/cockpit-e2e`; dependency checkouts, builds, launch manifests and evidence then stay under that directory.

Each acceptance scenario supplies a fresh `OSG_SIMULATION_NAMESPACE` to its
Cockpit service, gateways and devices. Their discovery directory is
`/tmp/opensimgear-<user-id>/gateways-<namespace>`, isolating local CDC endpoints
from any already running desktop and other acceptance runs. All participating
simulation binaries must support this optional namespace variable.

The first Cockpit scenario starts its normal native gateway service before the gateway,
then launches three JSON-configured devices. It verifies automatic enable/append,
configuration Apply & save, explicit changed-profile refresh, real input reports,
neutral offline ghosts, disabled online configuration, persistent Disable across
announcements and gateway restart, append-on-Enable, offline Forget and automatic
arrival of a forgotten identity. It also covers device/gateway crash recovery,
writer contention after Cockpit restart and independent process lifetimes.

Before the lifecycle scenario presses momentary contacts, it waits for current
active input on a settled published route, explicitly releases the exclusive
group and hat contacts, and confirms selector-center and released input state.
Each simulation contact command advances acquisition by 30 ms, exceeding the
10 ms release debounce. Publication alone does not establish button readiness:
contacts held during route initialization stay suppressed until released. The
held-input and Disable-neutrality assertions run after this released baseline.

The simultaneous-gateway scenario connects two gateway processes through one normal
Cockpit service. Each gateway has a device with the same identity, which verifies
that configuration caches, writer sessions, HID layouts and input reports are scoped
to the gateway. It checks aggregate discovery, rejected commands without a gateway
identity, independent errors/disconnects, and crash recovery while the other gateway
keeps its configuration writer and accepts saved changes. Correlated browser requests
also verify that a paused gateway cannot block status requests to its peer.

The manual-action scenario discovers the production device's RGB and boolean LED
descriptors, invokes them through the normal Rust adapter and reads applied state
from the independent device. It verifies unchanged HID descriptors/configuration,
no writer claim, argument and revision validation, stale boot rejection, disabled
HID and direct USB ownership, and isolation of identical device IDs on two
gateways. Dropping the action acknowledgement establishes uncertain state and
recovery through reconnect/readback without replaying the action.

The master-caution scenario connects the production event/action routes to the
independent DCS-BIOS UDP peer. It covers HID/direct USB exclusion, held startup,
short presses, gateway isolation, feedback loss, counter and aircraft changes,
fragmented exports, reboot, mission restart and one-time Stop release. Recovery
can update the LED before the worker establishes its new input baseline. The
fixture observes disarmed status during loss and waits for fresh active feedback,
`needsBaseline=false` and `armed=true` before injecting a valid new press, while
continuing to assert that historical presses never replay.

Native simulations execute the production configuration, flash journal, Cyphal
transport, registry and descriptor code. Loopback TCP carries CDC bytes and UDP
carries CAN frames. These tests validate protocol/report behavior; native streams
do not enumerate OS USB/HID devices. Physical USB/CAN acceptance remains separate.
