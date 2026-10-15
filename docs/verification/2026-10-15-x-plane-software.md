# X-Plane software validation — 2026-10-15

Issue: [cockpit #45](https://github.com/opensimgear/cockpit/issues/45).

Implemented in the isolated `codex/issue-45-xplane` worktree, starting from
`7e73564`. The primary checkout's existing changes were not included.

## Evidence

Checks ran in WSL Ubuntu using the installed Rust and Node toolchains.

| Check | Result |
| --- | --- |
| Native integration coverage runner | 326 passed, 1 existing ignored archive smoke test |
| X-Plane provider production line coverage | 84.74% (272/321) |
| X-Plane installation production line coverage | 96.05% (73/76) |
| Other integration modules | All exceed the existing 70% line gate |
| Frontend transport/helper suite | 154 passed |
| Games/integration/runtime-config coverage suite | 42 passed; coverage gate passed |
| Svelte check | 0 errors, 0 warnings |
| Vite production UI build | Passed |
| Windows native library cross-check | `x86_64-pc-windows-gnu` passed |
| Browser setup/profile acceptance | 3 passed: X-Plane plus existing DCS/ACR flows |

The browser flow covers choosing a secondary installation, setup readiness versus
waiting telemetry, version and UDP settings, portable indicator/display import,
validation/save, capture into the matching game profile, keeping activation explicit,
and 1440/768/390 px layouts. It uses authored gateway replies and never launches
a simulator or activates outputs. Desktop and mobile screenshots were inspected.

Native tests cover both version fixtures, Steam and manual copies, wrong-version
selection, full legacy-store migration and ID collisions, profile process identity
removal, setup override preservation, launch/process-exit coordination, catalogue
validation, actual local UDP subscriptions/replies, source filtering, port conflicts,
renew/unsubscribe, malformed/non-finite/duplicate/unknown indices, missing guards,
missing channels/engines, expiry, pause/replay, aircraft changes and fresh baselines.
Late packets with retired subscription indices are ignored without refreshing the
current session. Output acquisition uses the existing shared integration runtime.

Logs and screenshots are in the worktree's ignored `.research/` directory:
`integration-coverage/`, `frontend-coverage.log`, `transport-tests.log`,
`windows-check.log`, `ui-tests.log`, and `setup-ui-acceptance/`.

## Qualification still required

No real X-Plane 11 or 12 telemetry run has been recorded. Neither minimum build
compatibility nor a specific aircraft, native Windows UDP process association,
pause/replay/aircraft transitions, physical LED/display output or assembled hardware
has been qualified. Cross-compilation does not replace running on Windows.

The authored fixtures are labelled separately from simulator captures. Setup and
fixture traffic never set native verification. Native Linux/macOS process monitoring
and remote-simulator acquisition are explicitly unavailable. The existing ignored
DCS archive smoke test was not run; its pinned archive was unavailable locally.

To finish #45, record independent Windows runs for X-Plane 11 and 12 with exact
simulator build, aircraft and OS versions. Demonstrate one indicator and a numeric
display, then exercise wrong sources, unavailable datarefs, pause/replay, aircraft
switching, exit/restart and recovery. Record simulated versus physical devices
separately. See [the setup and channel guide](../x-plane.md).
