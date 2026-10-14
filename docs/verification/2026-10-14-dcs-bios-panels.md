# DCS-BIOS panel verification — 2026-10-14

Milestone 3 connects simulated radio panels to a synthetic A-10C DCS-BIOS peer
through the normal native Cockpit service, production module acquisition/action
callbacks and production gateway CAN/CDC routing. Physical boards use the same
contracts. The starter board exposes its fitted status LED and no display ports.
No hardware or live DCS validation was performed.

## Behavior verified

- Schema 2 saves up to 16 named input and 16 output bindings across eight known
  modules and four gateways. Catalogue searches return bounded normalized pages;
  command interfaces and output metadata come from the installed catalogue.
  Existing schema 1 master-caution settings remain usable until explicitly saved
  as schema 2. Neither saved version starts automatically.
- Momentary buttons preserve ordered press/release, maintained switches send
  absolute states, encoders send fixed or variable steps, and axes use calibrated
  device observations with availability flags and thresholds in command units.
  Input-bound panels require disabled/unpublished HID and inactive direct USB;
  output-only panels can keep their controllers enabled.
- Display callbacks advertise dimensions, encoding and byte budgets. Formatting
  pads the complete row-major surface, aligns text and truncates whole Unicode
  scalars or rejects overflow. The radio has a 7-cell ASCII frequency display,
  2-cell ASCII preset display and 16-cell UTF-8 display with a 63-byte limit.
  Typed readback confirms software application, not electrical operation.
- Complete advancing-counter export frames commit feedback atomically. Unknown,
  stale, malformed or unsupported-aircraft feedback applies configured fallback
  text or LED off. Frozen packets cannot restore freshness after a timeout;
  recovery requires an advancing counter and reconstructs current context.
- One worker per gateway prioritizes ordered inputs before another output. Each
  setter has one pending latest value. Feedback replacement coalesces setters;
  button edges and encoder steps remain ordered. A selectively lost setter reply
  is reconciled by readable state before another invocation.
- Startup, reboot, route/stream changes and event gaps establish current baselines.
  Historical events do not replay; a held momentary input needs release. Switches
  and available axes synchronize current absolute state. Slow reads recheck
  freshness and typed capabilities before sending commands. Stop neutralizes a
  previously forwarded button once and requests output fallbacks.
- Manual control of running bound outputs is guarded by gateway/module/action
  identity. Same module identities on different gateways remain isolated.

## Native load measurements

The radio acceptance fixture uses two independent gateway processes, each with
the same module identity, six inputs and eight total outputs. The slower panel
adds 40 ms to display writes. A 200 Hz synthetic feedback flood produces 886
complete frames over 4,855 ms, then all eight latest desired values must match
actual device action readback. Both output queues finish empty.

| Native measurement | Slower gateway | Other gateway |
| --- | ---: | ---: |
| Pending setter high-water | 4 | 4 |
| Superseded setters coalesced | 2,033 | 2,041 |
| Action latency p95 | 134.38 ms | 93.94 ms |
| Maximum input poll interval | 174.65 ms | 141.34 ms |
| Applied action throughput | 6.21/s | 7.85/s |
| Sampled-input-to-peer p95 | 179 ms | 151 ms |
| Sampled-input-to-peer maximum | 179 ms | 152 ms |

These are native software measurements on this host. They do not predict STM32
CAN/USB timing or live simulator latency. Input sampling-to-peer p95 is gated at
250 ms in the unloaded native fixture. Existing sealed command envelopes suffice
for this workload; no bulk protocol or negotiated batch capability was added.

## Tests and coverage

All 146 native tests, 79 frontend transport tests, legacy master-caution
acceptance and 31 radio scenarios pass. A sustained frozen-counter test holds
all six inputs disarmed and eight fallbacks stable for 1,001 ms (two freshness
windows) across 30 continuing frozen frames before checking recovery. CI requires every measured changed
production module to exceed 70% line coverage. Frontend action/profile helper
gates also require more than 70% branches and functions.

| Production module | Line coverage |
| --- | ---: |
| Shared actions | 99.65% |
| Shared panel events | 99.32% |
| Device actions | 96.67% |
| Device CAN configuration/client | 90.62% / 90.82% |
| Gateway CDC / configuration forwarder | 96.97% / 90.41% |
| Cockpit action validation / STM32 action routing | 97.86% / 93.85% |
| Cockpit DCS parser / legacy runtime | 97.44% / 86.44% |
| Cockpit event validation | 100% |
| Cockpit profile core / scheduler | 98.22% / 96.74% |
| Frontend schema 2 profile helpers | 86.30% |
| Synthetic peer | 100% |

The frontend profile helpers have 98.61% branch and 100% function coverage.
Meaningful regression coverage includes changed live encoder metadata, partial
and maximum-width strings, byte/cell encoding limits, unavailable axes, source
kind/HID exclusion, atomic export, uncertain writes, stale reads, independent
gateways and explicit startup/recovery behavior. ASan/UBSan native firmware
checks pass. Five device profiles and the gateway cross-build for Arm only.

Reproduce the software checks from Cockpit:

```sh
pnpm check
pnpm transport:test
pnpm actions:coverage
pnpm dcs:coverage
pnpm dcs-profile:coverage
pnpm ui:build
cargo test --locked --manifest-path src-tauri/Cargo.toml
python3 scripts/action-coverage.py --output-dir ../.research/action-coverage
python3 scripts/dcs-bios-coverage.py --output-dir ../.research/dcs-bios-coverage
OSG_E2E_TEST_OUTPUT=../.research/bootstrap-tests python3 -m unittest discover -s tests/e2e -p 'test_*.py'
python3 tests/e2e/run.py --sanitize --output-dir ../.research/cockpit-e2e
```

The E2E dependency manifest pins the coordinated device/gateway/common commits.
The runner clones dependencies into its output directory and requires no adjacent
checkout. CI uploads run JSON, resolved revisions and coverage reports.

## Visible interface check

Two independent radio fixtures were configured through the real profile editor
and native development bridge. Named catalogue choices created four inputs and
four outputs, including an output-only second gateway whose HID stayed enabled.
Save left the profile stopped. Disable displayed the existing reconnect/binding
warning; Start became available after the route refreshed.

Explicit Start showed all typed inputs Ready. Two encoder detents produced two
INC commands, a switch produced its current absolute state, a short button press
produced ordered 1/0, and a calibrated axis produced UHF_VOL 32768. Displays
showed preset 03, formatted VOL 50.0 and the second gateway frequency 251.000.
Paused feedback applied STALE/blank fallback and disarmed inputs. Three encoder
turns during loss/recovery did not replay. Global Stop worked without re-enabling
controllers. Readback and screenshot evidence is recorded in the local run.

Local artifacts: `.research/milestone3/{native-coverage,action-coverage,ui}` and
`.research/panel-outputs/cockpit-acceptance/dcs-radio-eoVkz1/`. Raw performance
and acceptance reports contain diagnostics, samples and final desired/applied
values. They are ignored build evidence; this record preserves the results.

Broader aircraft support, profile import/export, output-only enrollment and
physical qualification remain [Milestone 4](https://github.com/opensimgear/cockpit/issues/16).
Implementation tracking is [Milestone 3](https://github.com/opensimgear/cockpit/issues/15).
