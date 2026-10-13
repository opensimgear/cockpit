# DCS-BIOS master-caution verification — 2026-10-13

Milestone 2 implements a simulated A-10C master-caution button and LED path
through the normal native service, production gateway/module event routing and
device-owned LED actions. Current-board firmware uses the same logical button
and `led.status.set_mode` contract. Assembled STM32 hardware and a live DCS
installation have not been verified and remain deferred qualification work.

## Implemented behavior

- One saved profile resolves named `UFC_MASTER_CAUTION` and `MASTER_CAUTION`
  controls, aliases and export metadata from the installed DCS-BIOS `doc/json`
  directory. The simulation peer's authored catalogue uses synthetic addresses.
- Save and application restart do not start a binding. Start validates a current
  online CAN route, disabled/unpublished HID, inactive direct USB, a mapped
  button and a readable LED setter. Settings changes require Stop.
- The Rust background worker consumes ordered press/release events and export
  packets without depending on frontend polling. Freshness and recognized
  aircraft gate command delivery; advancing update counters gate export updates.
- New boot/stream/route, startup, simulator recovery and event gaps rebaseline at
  the current head. Retained historical presses do not replay. Held startup
  requires release before a fresh press. Disarm/Stop sends one release only for
  this binding’s previously forwarded press. Encoder pulse bindings are rejected.
- Missing/stale export, unsupported aircraft and Stop request LED off. Failed
  output confirmation remains visible; LED recovery reads state first.
- The bound LED is managed by the running integration. Global Stop remains
  available when its device is absent. Other manual outputs remain usable.
- Retained event overwrites report ring evictions, not a lost-command count.
  Recovered cursor gaps have a separate durable counter for the running session.

## Frontend verification

Commands run from `cockpit/`:

```sh
pnpm check
pnpm transport:test
pnpm dcs:coverage
pnpm ui:build
```

- `pnpm check`: zero errors and warnings.
- `pnpm transport:test`: 63 tests passed, including seven DCS profile helper
  tests and global DCS transport correlation/queue isolation.
- `pnpm dcs:coverage`: 86.75% line coverage, 100% branch coverage and 100%
  function coverage for `src/lib/dcs-bios.ts`. The command gates each metric
  above 70% and runs in CI.
- Production frontend build passed. Official Svelte documentation and autofixer
  checks completed for `DcsBiosPanel.svelte`, `ManualActions.svelte` and
  `App.svelte`, with no remaining autofixer issues or suggestions.

Tests cover default/saved configuration, momentary button bounds, compatible LED
selection, fresh/unknown LED labels, required HID/direct USB state, schema and
network bounds, and save/start flow. Transport tests ensure global responses
without a gateway identity preserve workspace inventory and remain independently
queued from slow gateway requests.

## Native and production-process verification

The native suite passed 111 tests, including 25 DCS core tests, four background
runtime cases, and event validation/routing coverage. Coverage measured with the
native instrumented test suite:

| Changed production area | Covered / measured lines | Line coverage |
| --- | --- | --- |
| DCS-BIOS core | 672 / 691 | 97.25% |
| Background DCS runtime | 406 / 465 | 87.31% |
| Panel event validator | 55 / 55 | 100% |
| Native action validation | — | 97.75% |
| Native action/event routing | — | 93.82% |

All four production-process scenarios passed in 49.12 seconds. The final
master-caution scenario, including one-time Stop release, passed in 6.42 seconds.
Private simulation namespaces separate these processes from existing desktop
discovery. Artifacts are under the parent workspace's
`.research/milestone2/e2e/` and `.research/dcs-bios-coverage/`.

Supporting verification passed: five shared native suites, thirteen module
native suites plus six module process cases, three gateway native suites and
full CDC/CAN acceptance, protocol integration, and all five firmware cross-builds.
Changed firmware line coverage was 99.14% for common events, 100% for the event
decoder, 90.62% for module event routing, 96.97% for gateway parsing and 90.41%
for gateway forwarding. All measured changed production areas exceed 70%.

CI enforces native production coverage individually for the DCS core, background
runtime and event validator through `scripts/dcs-bios-coverage.py`, alongside the
existing action gate.

## Visible UI acceptance

A Brave browser run used the ordinary Vite bridge and current Rust service in
private namespace `m2-ui-f9570bf2`, with a separate settings path and UDP ports
27778/25010. It selected the production master-caution simulation and completed
the visible Disable confirmation before saving and starting the profile.

Save left the profile stopped. Explicit Start displayed a fresh A-10C mission,
managed the manual status LED control and showed LED On after the peer's warning
was set. A physical contact press/release produced the exact ordered commands
`UFC_MASTER_CAUTION 1` / `UFC_MASTER_CAUTION 0`, cleared the simulated caution,
displayed LED Off and counted two sent commands. Global Stop on Gateways while a
second press remained held sent exactly one cleanup release; peer history was
`1, 0, 1, 0`, with device action readback `off` and stopped UI state.

Screenshots, DOM snapshots, settings and process readback logs are retained in
the parent workspace's `.research/milestone2/ui/`. Only this run's tab and
processes were closed. Live DCS and hardware were not used.

Website Markdown verification also passed: Astro checked 140 files with zero
errors/warnings/hints and built 56 static pages under
`.research/milestone2/website-build/`. Existing dependency/source-map/chunk and
site-content warnings remain unrelated to these Markdown changes.

The process fixture uses `simulation/dcs-bios/peer.mjs`,
`simulation/dcs-bios/catalogue/`, the production module's master-caution profile
and board-status output mode. It exercises normal adapters, ordered event pages
and UDP export/command handling. Test settings and local gateway discovery use
private paths/namespaces to avoid an existing desktop attaching to fixtures.

## Qualification boundary

Software action readback proves callback application, not electrical LED
operation. The synthetic peer establishes neither a real DCS installation nor
aircraft behavior. Physical button wiring, STM32 LED/USB/CAN, Windows game
bindings and simulator installation/reconnect qualification remain deferred and
do not block the simulated milestone. Displays, encoder simulator bindings,
wider aircraft profiles and their hardware acceptance belong to Milestones 3–4.
