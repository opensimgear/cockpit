# Feature 78 acceptance map

This maps the revised system-state scope of [#78](https://github.com/opensimgear/cockpit/issues/78)
and D0–D6 to production code and automated acceptance. Profiles are not required
for inspection. Existing assignment metadata is read only when disclosing Forget
impact. Native hardware observations and fixture limits are recorded in the
[running-Cockpit verification](2026-10-18-unified-devices.md).

| Story and requirements | Production authority | Direct acceptance evidence |
| --- | --- | --- |
| D0 #90: identities, categories, evidence, provenance, multiple connections, separate presence/capture/enablement/visibility/ownership, freshness, actions and references | `src/lib/system-inventory.ts`; native resource identity facade; `system-inventory-providers.ts` | Registry/provider tests cover all categories, confirmed versus ambiguous identity, simultaneous USB/CAN, independent configuration/input ownership, unsupported capabilities, paged snapshots, per-provider failures and restart |
| D0: provider/route authority and persistence | Existing native stores retain device configuration, pairing, bindings and writer ownership; the presentation registry stores remembered observations/tombstones | Identical IDs across renderers; durable registry reload; stale observations become unknown; quota failures roll back Forget; no driver/configuration writes during inspection |
| D1 #91: complete physical, gateway, virtual, integration, companion and visibility adapters | Controller and USB pages; gateway workspace; game catalogue/state; companion status; `system_inventory` native supplement | Paged-controller/USB/resource/native regressions; fixture standalone/output-only and dual-transport modules; actual Windows controller/vJoy/integration/companion/HidHide inventory |
| D1: verified identity without duplicate physical counts | Native serial/USB interface/report association plus proven resource grouping; exact PnP instance/nearest physical USB parent; canonical alias migration | Cross-gateway/Pico isolation, lookalike rejection, gateway HID deduplication, sibling collections/parent rules, outage/recovery alias regressions; unmatched identities stay separate |
| D1: virtual availability and creation provenance | Read-only SDK inspection plus configured registry inventory and exact committed provisioning-journal descriptor match | Native free/busy/missing/unknown/SDK-failure and provenance tests; installed vJoy observations; unused absent slots are excluded and known configured missing targets retained |
| D2 #92: observed state, freshness, process visibility scope and independent provider errors | Shared component summaries and details; native PnP/HidHide; existing type-specific visibility detail | Connected+Hidden, disabled versus disconnected, pending pairing, unconfirmed integration endpoints, unavailable backend, stale ownership clearing, native parent bus evidence, inverse/application-scope diagnostics and graph/List status parity tests |
| D2: lifecycle without inspection mutations | Shared bounded refresh, provider/session/page consistency checks, disposal epoch fencing and guarded native reads | Hotplug/reconnect/driver/service/pairing/restart and delayed-response tests; fixture and actual-native request logs contain no inspection-triggered acquisition, configuration or visibility writes |
| D3 #93: navigation and shared presentation state | App removes Topology/Gateways sidebar entries; `SystemDevices.svelte` owns the responsive List tile grid/Topology, filters, counts, selection and details | Running fixture checks absence of both navigation entries, multi-column tiles, readable muted offline state, hidden-present distinction, stable card styles during live updates, all type counts, combined connection filters, remembered presentation, shared selection and return from module configuration |
| D3: existing gateway/type-specific actions and accessible partial states | Dedicated details replace the inventory and reuse gateway module controls, HostControllers, direct USB setup/actions, integration settings and companion editors | Running details/Back with restored filters and selection, automatic updates without Refresh controls, gateway Disconnect/Connect and configuration round-trip; compiled USB claim/Apply/Save/release/action/lifecycle tests; no external-controller firmware controls; keyboard buttons and explicit errors/empty states |
| D4 #94: every component and evidence-backed physical/software connections | `SystemTopology.svelte`; deterministic `system-topology.ts`; same component IDs/state as List | Full inventory parity; dual USB/CAN independent edges; no remote-flag fabricated USB; unknown/offline nodes; distinct physical/software/configured/last-known labels; upstream filter context; no invented CAN daisy-chain |
| D4: stable layout, pan, zoom, fit and equivalent actions | Wrapped deterministic layers; keyboard/drag pan, anchored zoom, explicit Fit and shared details | Graph geometry/order/cycle/parallel-edge tests; compiled full-state summary parity; running keyboard/zoom/fit and visual screenshot inspection with the actual native inventory |
| D5 #95: single/bulk eligibility, confirmation, type-specific effects, reference disclosure and one-shot removal | Shared confirmation; fresh provider eligibility; existing controller/module/companion operations; local virtual/gateway metadata cleanup | Real native-facade UUID references, gateway child/assigned companion reference disclosure, online/hidden/owned rejection, concurrent reconnect, partial bulk failure and no ambiguous mutation replay |
| D5: gateway route cleanup and durable rediscovery behavior | Transactional registry tombstones prune only selected gateway/sole-route descendants; preserve independent connections and user bindings | Restart/stale-snapshot suppression, genuine rediscovery, independent USB/other-gateway retention, recursive orphan cleanup, storage-failure rollback and running unavailable-gateway cleanup |
| D6 #96: assembled feature and honest qualification | Actual dev desktop plus frontend bridge; existing native ownership/qualification gates remain authoritative | Deterministic running-Cockpit workflow, passive actual-native inventory workflow, native/controller/virtual/service coverage, frontend checks/builds and existing transport/profile regression suites |
| D6: evidence and user/architecture guidance | `README.md`, `docs/system-inventory.md`, updated host-controller guide and this verification pair | Explicit fixture/native split, real device counts/binary hashes, reproducible commands, and documented unqualified hardware combinations; issue acceptance/CI links record delivery status |

## Validation commands

- `pnpm system-inventory:test` and `pnpm system-inventory:coverage` gate the shared
  contract, provider projection, topology geometry and compiled Svelte handlers.
- `pnpm check`, `pnpm ui:build`, `pnpm transport:test` and
  `pnpm profile-workspace:test` check types/build and existing workflows.
- `pnpm devices:ui` uses the running dev Cockpit with deterministic native replies;
  `pnpm devices:native:ui` uses its real native service without interception.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` runs native regressions.
  Existing controller, virtual, companion and module coverage scripts include the
  changed production modules with their normal greater-than-70-percent gates.

CI conclusions belong to the exact pushed revision, not an earlier failed or
cancelled run. The issue delivery records link the final accepted runs. Fixture
success never certifies absent physical hardware, driver output or game behavior.
