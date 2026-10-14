# Milestone 4 simulator integration software verification

Verified on 2026-10-14 using independent gateway/device simulations and their
production action, configuration, event and CAN contracts. This record covers
software. Physical LEDs/displays, electrical CAN/USB behavior, real DCS, Windows
runtime acquisition and live Assetto Corsa Rally versions remain deferred.
PCB versions remain 0.1.0.

## Implemented behavior

- Portable schema-1 profiles separate provider/control/channel identities and
  logical device roles from local gateway assignments and connection settings.
  Import previews do not acquire outputs or start runs; Validate and explicit
  Save/Start check installed catalogues, descriptors, routes and output capabilities.
  Catalogue/route changes require revalidation. Saved settings never autostart.
- DCS-BIOS uses supported named controls from installed aircraft modules. Existing
  saved DCS schemas remain available. New portable output bindings require
  advertised device-enforced endpoint ownership; older profiles retain their
  compatibility guard.
- Assetto Corsa Rally supplies RPM, gear, speed, maximum RPM and derived rev ratio.
  Threshold and RGB-band formatting binds these channels to advertised outputs,
  including the current board LED. Other games can add independent providers.
- Endpoint leases protect aliases and competing clients, with bounded temporary
  overrides, latest-value Resume, typed unknown/stale fallback, device-side expiry,
  transport-loss invalidation and explicit Stop. Per-gateway workers preserve
  independent progress while telemetry acquisition continues.
- Output-only modules join ordinary inventory/configuration without publishing
  an empty HID controller. Native STM32 and Pico debug devices use the same
  ownership/action contract.

## Automated checks

The full native suite passes 199 library tests and one actual-service process
regression. The frontend suite passes 118 tests,
Svelte/TypeScript checking reports zero errors/warnings and the Vite production
build passes. Five dependency-bootstrap behavior tests pass. Coverage gates use
production code, excluding inline native test modules and generated/vendor code;
they enforce more than 70% for each maintained measured module.

| Native production module | Line coverage |
| --- | ---: |
| Portable profile validation/formatting | 98.24% |
| Integration lifecycle/runtime | 94.96% |
| Rally parser, sessions and telemetry | 98.80% |
| Portable Windows acquisition helpers | 100.00% |
| Endpoint lease adapter validation | 98.71% |
| Runtime configuration | 81.03% |
| STM32 action/debug adapter ranges | 93.74% |
| Pico adapter | 86.28% |
| Local gateway adapter | 88.40% |
| DCS export processing | 97.85% |
| DCS runtime | 86.44% |
| DCS profile | 98.20% |
| DCS per-gateway scheduler | 94.91% |

Frontend profile helpers measure 87.83% line coverage, 96.59% branches and
98.41% functions; action and existing DCS helper gates also pass. Common C actions
measure 99.53%, device actions 97.06%, route handlers over 91%, runtime config
100%, gateway/registry handlers 97.01%/100%, Pico virtual devices 98.95% and debug
core 100%. Device USB descriptor coverage is 73.53%, including CDC-only export.

Eight fresh-clone, sanitizer-enabled process acceptance scenarios pass:
radio peer; radio panel with two gateways and ordered stress; portable Rally
outputs; portable DCS master caution; controller lifecycle/restarts; multiple
gateways; manual actions; and legacy DCS master caution. Portable Rally uses
three output-only modules across two gateways, RGB/boolean/current-board LEDs,
competing clients, overrides, unknown maximum RPM, pause/freeze fallback, reboot
and host-loss expiry. Portable DCS verifies claims, HID exclusion, ordered short
presses, held startup/recovery, Stop's single owed release and board LED fallback.

Native regressions also hold telemetry fresh during a 5200 ms blocked event read
and inject a confirmed owned-write rejection. Neither can forward late inputs
using an expired/rejected output claim. Device regressions cover persistent route
loss during request timeouts and same-tuple CAN reassignment, including cached
mutation replies. Reviews found and corrected these gaps before final acceptance.

The actual Windows-only provider source compiles for `x86_64-pc-windows-gnu`.
Host coverage exercises bounded copy/process-validation helpers; it does not
measure Windows FFI execution. Six STM32 device profiles, the gateway and Pico
RP2040 compile successfully. These builds are compile-only and were not flashed.

The visible browser UI passed import preview without assignments/autostart,
exact target assignment and capability refresh, catalogue loading, rev-light
creation, Validate/Save/revalidation/Start, green and red RPM feedback, a blue
override, explicit Resume before expiry, stale fallback while frozen packets
continued arriving, advancing recovery and Stop with unowned neutral outputs.
Canonical export preview excluded local connections, assignments and device IDs.
The browser file-download event was not confirmed; the visible export and its
canonical JSON were inspected. Screenshots, accessibility snapshots and native
device readbacks are under `.research/milestone4/ui/`.

## Reproduction and evidence

Run the maintained commands from Cockpit:

```sh
pnpm check
pnpm transport:test
pnpm integration:coverage
pnpm actions:coverage
pnpm dcs:coverage
pnpm dcs-profile:coverage
pnpm debug:coverage
pnpm ui:build
python3 scripts/integration-coverage.py --output-dir ../.research/integration-coverage
python3 scripts/action-coverage.py --output-dir ../.research/action-coverage
python3 scripts/dcs-bios-coverage.py --output-dir ../.research/dcs-coverage
python3 -m unittest discover -s tests/e2e -p 'test_*.py'
python3 tests/e2e/run.py --sanitize --output-dir ../.research/integration-e2e
```

The committed E2E dependency manifest pins immutable published revisions. Final
local evidence is under `.research/milestone4/`: `cohort-e2e/` includes dependency
resolution and per-scenario results; `integration-coverage-final/`,
`action-coverage-final/` and `dcs-bios-coverage-final/` contain annotated coverage
and complete native test logs. Firmware review/build/coverage records and the
Windows compile log are siblings. CI uploads the same maintained runner outputs.

Authored byte fixtures and pinned developer provenance are in
[the Rally fixture guide](../../tests/fixtures/acr/README.md). The shared-memory
layout is developer-derived and mapping names cannot identify the producer;
[the integration guide](../simulator-integrations.md) records source selection,
freshness and qualification limits.

## Corrections from merge review

The browser service previously retained a 17,408-byte envelope limit while the
manager and HTTP bridge accepted 64 KiB requests. A legal full-size authored
telemetry frame could therefore return an uncorrelated error and terminate the
bridge's service. The service now allows the complete payload plus 1 KiB for
correlation framing. A regression launches the actual native executable and
checks larger telemetry pages, an exact-limit payload, correlated over-limit
rejection and a subsequent successful request on the same process.

The UI also copied integration status from cached per-gateway inventories on
every workspace notification. A slow or offline gateway's old running snapshot
could overwrite the confirmed Stop response. Dedicated global requests and
status polls now supply integration status; cached inventory updates only update
gateway/device state. The actual workspace callback's before/after reproduction,
frontend checks and official Svelte analysis confirm the correction.

Local STM32 gateway identities now canonicalize during profile validation, while
Pico serial spelling remains intact. A valid uppercase STM32 assignment can
Save/Start, apply an owned override and Resume using the same route identity.
Case aliases cannot bind the same physical endpoint twice through different roles.

Independent firmware reviews passed fresh common/device sanitizer suites and
the gateway/Pico/real-CAN stress checks. Protocol documentation now distinguishes
stored output-only sentinels, raw CDC values and Cockpit normalization; the Pico
guide identifies the all-inputs preset's capacity explicitly. Merge review
evidence is under `.research/milestone4-review/` in the workspace.
