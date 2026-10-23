# Review acceptance — 24 October 2026

Integration includes widget authoring through `08083f4`. Review fixes update
generated ESP32 fixtures, preserve deterministic fixture/license line endings on
Windows, and replace retired schema-2 acceptance inputs with compiled panels.
Compiled protocol probes are test fixtures, not additional shipped controls.

The isolated companion metadata directory now resolves the system temporary base
before applying the existing prohibition on linked storage. This addresses the
macOS `/var` alias without relaxing checks inside the selected storage directory.
macOS execution remains a CI check, not a locally observed pass.

Authoring admission helpers and product/test seeds are separate modules below
500 lines. Coverage includes the extracted modules with the unchanged 71% gates.

## Observed automated checks

- Complete frontend regression: 1,143 passed.
- Complete Windows native library regression: 1,154 passed, eight ignored.
- Type checks: zero Svelte errors or warnings in Cockpit and browser companion.
- Companion frontend coverage gates passed. Reorganized compiler coverage:
  99.90% lines, 95.02% branches, 96.92% functions.
- Native companion coverage gates passed, including authoring admission 81.87%
  and extracted helpers 73.61% production lines.
- Desktop coverage gates passed; recording preview measured 81.63% lines,
  88.64% branches and 90% functions.
- Acceptance decoder/transport Python tests: 20 passed.
- Browser companion regression: 34 passed.
- Deterministic compiled acceptance fixture verification passed.

Native suites require normal localhost and filesystem access. Sandbox-denied
runs were not counted as product failures or successful acceptance.

## Actual running development Cockpit

The authoring check creates a desktop-only profile, captures a key after native
Stop, saves/reopens/exports/imports it, and records a synthetic preview. It sends
no OS input. Capture picker focus survives a refresh; the captured HID 79 remains
selected across two further refresh cycles before Update desktop actions.

The user-approved receiver experiment recorded US/German layouts, ordinary and
extended keys, modifiers, five mouse buttons, wheel and pointer input. The worker
experiment recorded ownership, scope, leases, parent death and worker death.

The real Cockpit/WSS/receiver check passed shared Ctrl ownership, encoder tap
bursts, pointer integration, focus pause and neutral rearm, macro cancellation,
source lease loss and cleanup. It uses zero virtual controllers. Receiver input
observations are distinct from SendInput counts and from game acceptance.

The isolated native WSS feedback check passed two-peer isolation, typed local
feedback, stale receiver behavior, stop/reacquisition and 64-channel feedback
larger than one transport frame. This is host feedback, not simulator confirmation.

The separate two-companion mouse ownership check timed out during Apply; its
wrapper cleanup completed without errors. That acceptance remains unresolved.

## Manual observations and remaining gates

The user reported that Toggle Preview requested state toggles while confirmation
remains independent. The user also captured physical Right Arrow as HID 79 with
no profile active, then observed it revert to HID 44. The delayed native UI test
reproduced that defect before the fix: parent refreshes reset an equal-data draft.
The editor now synchronizes only when authored endpoint content changes. The
regression passes against the running Cockpit; physical retest is pending.

Physical-keyboard overlap, higher-integrity/session transitions, game-observed
response, optional-driver-free deployment and installed-release checks are not
established here. Keep their review gates open until observed. The historical
debug-only production-gate assumption is superseded by current Windows code;
these results do not imply the entire original release matrix passed.
