# Automatic game integration setup verification

Stories [DCS #34](https://github.com/opensimgear/cockpit/issues/34) and
[ACR #35](https://github.com/opensimgear/cockpit/issues/35), verified on 2026-10-14
with Rust 1.90.0, Windows MSVC, Linux/WSL, Node 26.11.1 and Microsoft Edge 155.

## DCS setup

Setup downloads the official DCS-Skunkworks 0.11.7 runtime archive, verifies its
8,234,988-byte size and SHA-256
`3272bc07210fa6c560f115cfb5dc8881accf7af5e7a5e218e2c46e1fd89e3292`, and checks the
actual runtime, catalogue and export loader before reporting Ready. The real
archive smoke test runs separately from authored installation fixtures on both
Windows and Linux. CI downloads that exact release and runs the smoke test inside
the setup coverage gate.

Regression cases cover Saved Games selection, installed-game subtree rejection,
unknown module/configuration preservation, bounded archive extraction, loader
idempotence, existing export bytes and line endings, cancellation, recovery,
external edits and invalid local connection overrides. Setup checks the DCS
process again at write boundaries after downloading/staging.

The isolated Windows service check reused the actual official package in a scratch
Saved Games folder, added one loader to an export with an existing BOM/CRLF prefix,
repeated setup, and restarted the service. DCS remained Ready with live data
Waiting; setup never started an integration. Recovery settings and game settings
used a private namespace and temporary paths.

## ACR setup

ACR setup prepares native shared-memory settings without installing a plugin or
writing game files. Closed-game setup remains configured and unverified; restart
preserves that distinction. Profiles validate and save without a PID, while
manual Start requires an actual process and Games resolves it at launch.

Native verification binds an advancing compatible read-only stream to the actual
runtime executable fingerprint, creation epoch and lifetime. Authored probe tests
cover baseline/progression, pause/replay/off, stale packets, optional maximum RPM,
layout/access failures, competing producers and executable changes. The portable
probe test seam always returns nativeVerified=false; only the Windows source can
qualify native data. Synthetic samples never establish native readiness.

Windows discovery found the installed Steam DCS and ACR executables. Both games
were closed. The read-only ACR check correctly remained unverified and requested
a live driving session. The service smoke check launched no game, opened no serial
port and changed no real game files.

## Automated checks

- Svelte/TypeScript check: zero errors and warnings.
- Frontend and transport regression suite: 145 passing tests.
- Game/integration/configuration helper coverage: 39 passing tests; aggregate
  85.65% lines, 96.04% branches and 96.00% functions. Games helpers measure 86.25%
  lines; integration profile helpers measure 87.88%.
- Vite production UI build: passes, 161 modules.
- Linux and Windows native desktop/service suites: pass. Exact final test counts
  and setup production coverage are recorded below.

- Linux native suite: 297 library tests and two service tests pass.
- Windows native library suite: 285 tests pass; service binary test passes.
- The official archive test is intentionally ignored in ordinary suites and runs
  separately with `OSG_DCS_SETUP_ARCHIVE`: one passing test on each platform.

Final Linux production line coverage from `scripts/setup-coverage.py`:

| Module | Covered lines | Coverage |
| --- | ---: | ---: |
| DCS installer | 686 / 813 | 84.38% |
| Game integration setup coordinator | 654 / 764 | 85.60% |
| ACR setup/probe orchestration | 330 / 362 | 91.16% |

Each new module exceeds the 71% gate. Coverage includes the real release archive
path and the shared probe orchestration, while conditional Windows APIs are
qualified by Windows compilation/tests and the scratch-service check. Linux
coverage does not measure Windows operating-system branches. Logs and annotated
coverage remain under `.research/setup-coverage`.


## Browser acceptance and limits

`tests/ui/game-integration-setup.mjs` passes two browser scenarios with explicitly
authored gateway replies. It covers folder ambiguity, setup/progress/cancel/
recovery, ready versus waiting, native versus synthetic readiness, optional
channels, prepared integration settings, and the closed-game ACR save/capture/
activate flow without an invented PID. Layouts at 1440, 768 and 390 px were
visually inspected with no horizontal overflow or browser errors. Screenshots
remain under `.research/setup-ui-acceptance`.

These browser fixtures and scratch-service checks do not qualify a live DCS
mission, live ACR driving, real UDP multicast delivery or physical hardware.
Live-game and USB/LED/display/CAN qualification remain outstanding. The ACR layout
is developer-derived; named mappings alone cannot establish their producer, so
known competing Assetto processes are rejected during probes and ongoing reads.
