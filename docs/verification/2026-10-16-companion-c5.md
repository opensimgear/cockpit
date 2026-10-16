# Companion C5 implementation verification - 16 October 2026

Issue: [#43](https://github.com/opensimgear/cockpit/issues/43).
Native qualification: [#67](https://github.com/opensimgear/cockpit/issues/67).

## Delivered behavior

The shared Windows/Linux/macOS/Android/iOS panel renderer receives identity-bound,
typed visual feedback independently of its ordered input stream. Publication
metadata validates channel/output identity, value type, unit and range. Native
snapshots/deltas retain source age, provenance and freshness. Reset, resync,
rendering, telemetry updates and navigation have no input authority.

Host routes read requested or accepted source state, acknowledged feeder state,
or supported native integration telemetry. Recording is not submission. ACR
synthetic data remains Local; fresh supported simulator-decoder data can be
Game confirmed. Missing, stale or invalid values become unavailable. Semantic
source revisions prevent a changed integration from silently reusing a binding.
C5 inspection replies are capped at 130,048 bytes inside the existing 131,072-byte
bridge response budget; the maximum-shape test preserves all stable IDs/bindings,
128 controls, 64 outputs and existing 120 UTF-16-unit metadata. Full bounded
snapshots support all 64 maximum-size text feedback channels through
existing ordered chunk transport; physical frames remain capped at 16 KiB.

Schema 2 source-only integrations decode telemetry with no module/output lease.
DCS-BIOS boolean, numeric and string outputs are read through the production
parser. Schema 3 game profiles/journals optionally capture immutable companion
assignments. A native expected-state comparison preflights and atomically takes
owned assignments, preserves interrupted-session baselines, reports external
conflicts and restores only its own expected state. Legacy schemas retain their
meaning. Save/import/capture and host restart do not start game output.

Companion protected storage is now opened on explicit companion use rather than
in the gateway constructor. This removes synchronous Keychain/TLS setup from
unrelated stdio startup; it addresses the startup path implicated in the previous
macOS headless CI timeout. Remote CI must confirm that diagnosis on macOS.

## Local validation

Validation uses a clean archive of `2e7256a` plus only C5 changes, in isolated
Windows and Linux workspaces. Other chats' uncommitted desktop/game work is
excluded and preserved. Tests use in-memory adapters, simulated peers, injected
UDP telemetry and recording/fake feeders; they do not alter controller drivers,
HidHide policy or physical games.

- Frontend local CI passed: Svelte/TypeScript checks, transport regressions,
  controller/mapping/action/DCS/debug/integration/game coverage gates, production
  Cockpit build, companion checks/tests and renderer build. Companion suites:
  69 contract, 61 designer, 18 renderer, 12 library, four host-helper, three
  feedback-helper and 24 app tests. New feedback-state/presentation/helpers have
  100% line coverage. Games has 45 passing frontend tests and 85.71% lines.
- Native Windows host: 364 tests passed, one existing official-DCS-archive test
  ignored. Linux host: 374 passed, one ignored. Windows desktop/private service
  compiled; native companion lifecycle suites passed five tests on each OS.
- Companion service: Windows 63 tests passed; Linux 54 passed with one explicit
  native Secret Service qualification ignored. All 11 service modules meet 71%
  production line coverage (74.76%-100%). Desktop operator coverage is 92.73%.
- Mapping host: 66 tests passed. All eight production coverage gates passed;
  companion routes 89.82%, router 87.42%. The acknowledged-submission test uses
  a fake feeder and does not qualify live controller delivery. A pre-existing
  route-loss test now advances its lease clock explicitly; coverage execution
  is serial, avoiding unrelated CPU scheduling expiry.
- Native integration coverage passed on Linux for all 15 modules, including
  integration profile 97.27%, runtime 95.01%, games 93.53%, game mapping 97.92%,
  companion capture DTO/owner 100% and feedback router 97.11%. The complete
  integration gate runs on Linux, matching the CI workflow.
- DCS native coverage passed: decoder 97.90%, runtime 86.52%, panel events 100%,
  profile 98.18%, profile runtime 94.91%. Five new integration tests exercise
  source-only synthetic/DCS behavior, typed UDP decoding, freshness/Stop clearing,
  semantic revision pins and zero physical gateway reads/writes/leases.
- Seven feedback acceptance helper tests passed. Thirteen private-service to
  two pinned-WSS peer checks passed: typed injected DCS numeric/text data,
  synthetic Local state, requested/accepted distinction, spoof/old-session/output
  rejection, reset/resync input invariance, stale/unknown, disconnect/reconnect/
  revoke isolation and immutable capture. Both snapshot and delta were observed;
  a 16,790-byte logical snapshot used physical frames capped at 16,384 bytes.
  This harness is also wired into Windows CI with its evidence uploaded.

## Qualification boundary

[#67](https://github.com/opensimgear/cockpit/issues/67) retains real Android/iOS and
Windows/Linux/macOS client feedback, real simulator/game provenance, saved game
bindings, captured-assignment crash/recovery and measured multi-peer load. Its
manual criteria remain unchecked. Companion build CI covers unsigned development
packages, not signing, native lifecycle or physical delivery qualification.
Existing #54-#62/#66 qualifications remain open; Linux/macOS host output #48/#49
remain deferred. No mobile authentication, virtual-output, provisioning, hiding or
script gate is enabled by C5.