# Unified Devices inventory verification

Feature #78 and stories #90–#96 consolidate system inspection in Devices, with
shared List and Topology presentations. Profile authoring, activation and profile
attribution are deferred. Reading existing saved assignments before a Forget
confirmation only discloses retained references; it does not activate or edit them.

## Running Cockpit with deterministic providers

`tests/ui/system-devices.mjs` drives the development Cockpit served at
`http://127.0.0.1:1420`. Its Playwright browser installs fixture IPC before
navigation. The test uses production Svelte components, transport queues,
inventory projection, remembered records, topology and confirmation handlers.
It supplies deterministic provider observations instead of native hardware.

The fixture inventory contains 16 components: two gateways, four physical OSG
components, three third-party controllers, two virtual targets, two integrations
and three companion installation/assigned-instance records. A verified USB/CAN
module and its published HID collection share one identity; a matching module ID
on another gateway remains separate. The fixture also includes standalone and
CAN output-only modules, connected hidden pedals, offline controllers, a busy
external virtual target and an unavailable remembered virtual target.

| Workflow | Automated evidence |
| --- | --- |
| Complete shared inventory | Identical List/Topology component IDs, no duplicate verified sightings, complete controller/USB/shared-resource/native paging |
| Observed state | Hidden and connected remain separate; virtual ownership and creation provenance are independent; unknown/unavailable records remain visible |
| Partial-provider lifecycle | Controller failure retains independent providers; recovery rejoins the PnP observation without retaining a duplicate physical component |
| Shared presentation | Search, type/state/transport filters, type-aware counts, selection and details survive view switches; topology retains upstream context; remembered mode survives page reload |
| Gateway workflows | Disconnect/connect, module management and configuration navigation remain reachable from Devices; returning retains search and selected gateway |
| Accessible interaction | Native buttons, keyboard graph panning/fitting, zoom controls and type-appropriate shared actions |
| Single Forget | Explicit virtual metadata confirmation explains OS device/layout/driver/binding retention; existing saved assignment references are disclosed |
| Bulk Forget | Confirmation revalidates a reconnected controller; a separate failed removal is reported and sent once; independent failures do not remove other components |
| Durable cleanup | Unchanged stale controller/virtual snapshots stay suppressed across refresh and reload; genuine reconnect can reappear |
| Unavailable gateway | Confirmation explains local versus remote limits; stale gateway/sole-route CAN children disappear while an independently observed USB device remains |
| Inspection isolation | No configuration, visibility, profile activation or runtime acquisition mutations; no unexpected IPC or browser exceptions |

Local screenshots and command evidence are retained in
`.research/system-devices-browser/`. Those files are generated observations and
are not committed as hardware qualification.

## Actual native provider observations

`tests/ui/system-devices-native.mjs` drives the production UI through its ordinary
native development bridge at `http://127.0.0.1:5173`, with no IPC interception.
It reads actual native providers, verifies every controller, virtual target,
integration and paired companion is represented, and compares all rendered IDs
between List and Topology. It checks that the recorded HTTP requests contain no
configuration, visibility, cleanup or runtime acquisition mutations.

The Windows observation at **2026-10-18 16:05 Europe/Berlin** contained:

| Native observation | Count and interpretation |
| --- | --- |
| Controller inventory | 12 remembered/current controller records; five were connected physical controllers, including both VKB Gladiators and the T-Rudder, all with PnP-backed USB relationships |
| Virtual devices | Three installed, free vJoy slots, including observed Cockpit creation metadata |
| Integrations | Four known catalogue entries, including unavailable X-Plane 11 and independently reported setup/health states |
| Companions | Three paired installations and three assigned panel instances, all retained offline |
| HidHide | Three unmatched retained PnP rules; kept separate because no current controller identity proved equivalence |
| Shared inventory | 28 component IDs matched exactly in List and Topology; eight components reported connected |
| Provider limitation | The existing desktop owned companion listener `127.0.0.1:54437`; the second service reported its bind failure while preserving all companion records |

Evidence is under `.research/system-devices-native-browser/`, including
`evidence.json`, `native-system-list.png`, `native-system-topology-initial.png`
and `native-system-topology.png`.
The observed service copy was
`.research/feature78-native/latest/osg-gateway-service.exe`, SHA-256
`5aac033ff0146cbb20322cec2aaf9fe6b7dc5fbee1fb1c4f4b1ec9f81d43f1d0`.
This identifies the measured executable, rather than claiming later source edits
were present in that native binary. The UI uses the current development sources.

Screenshots were opened for visual inspection. The 28-node topology uses bounded
six-row sibling groups, starts at a readable 80% zoom with keyboard/pointer
panning, and fits the entire rig at 55% when Fit is selected. Labels, connection
types, current versus last-known links and type-aware counts remain distinct.
The final fixture and native runs passed together with no browser exceptions.

The final fixture evidence JSON has SHA-256
`9cecd1477c591848887678ce9b90f680caee2ca8852116354367d68c9cd6ab86`;
the final native evidence JSON has SHA-256
`a16c8ebeb5eedd4a6c42975e3d5275b2ddae4db7f22faa0413309e06ae0c313e`.

## Reproduction

Start the usual development Cockpit for fixture UI checks. With Playwright
available, run `pnpm devices:ui`; `OSG_DEVICES_UI_URL` overrides the default 1420
endpoint. `OSG_PLAYWRIGHT_MODULE` can point to an installed Playwright package and
`OSG_BROWSER_CHANNEL=msedge` selects the existing Edge installation on Windows.

For passive native checks, set `OSG_GATEWAY_SERVICE` to an absolute current debug
service executable and start `pnpm web:dev`. Run `pnpm devices:native:ui` against
the resulting native bridge; `OSG_DEVICES_NATIVE_UI_URL` overrides port 5173.
Keep actual native provider evidence separate from intercepted fixture runs.

## Qualification limits

No actual OSG gateway, standalone USB module, CAN module or dual-transport OSG
device was attached for these runs. Their discovery, correlation, output-only
behavior and gateway lifecycle evidence is deterministic fixture/native-unit
coverage. The native browser run proves current provider observations and UI
parity, not physical output, OS/game delivery, pairing/revocation on a real
companion, hotplugged OSG hardware, driver installation or release qualification.
Existing qualification gates and follow-up issues remain authoritative.

CI repairs made during this feature keep those gates intact: the release checker
understands the existing Windows debug-only exact opt-in, aircraft fixtures use a
regular temporary parent on macOS, and the UDP baseline fixture supplies bounded
advancing live data until admission completes. No production deadline or native
qualification setting was relaxed.
