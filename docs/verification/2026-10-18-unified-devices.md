# Unified Devices inventory verification

The observations below record the earlier complete-provider presentation. The
later user-requested active-integration/paired-companion projection supersedes
its visible counts; see [final acceptance](2026-10-18-devices-final-acceptance.md).

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
| Observed state | Hidden and connected remain separate; virtual ownership and creation provenance are independent; unknown/unavailable records remain visible; graph summaries retain the same full state in accessible text and tooltips |
| Partial-provider lifecycle | Controller failure retains independent providers; recovery rejoins the PnP observation without retaining a duplicate physical component |
| Shared presentation | Search, type/state/transport filters, type-aware counts, selection and details survive view switches; topology retains upstream context; remembered mode survives page reload |
| Gateway workflows | Disconnect/connect, module management and configuration navigation remain reachable from Devices; returning retains search and selected gateway |
| Companion lifecycle | Disconnect retains last-known network links; USB-only connections gain no fabricated network link; pending pairing is visible as an unverified request and is never automatically approved |
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

The Windows observation at **2026-10-18 17:48 Europe/Berlin** contained:

| Native observation | Count and interpretation |
| --- | --- |
| Controller inventory | 12 remembered/current controller records; five were connected physical controllers, including both VKB Gladiators and the T-Rudder, all with PnP-backed USB relationships |
| Virtual devices | Three installed, free vJoy slots, including observed Cockpit creation metadata |
| Integrations | Four known catalogue entries, including unavailable X-Plane 11 and independently reported setup/health states |
| Companions | Three paired installations and three assigned panel instances, all retained offline |
| HidHide | The exact parent USB rule joins its saved HID controller; two unmatched retained PnP rules stay separate because no identity proved equivalence |
| Shared inventory | 27 component IDs matched exactly in List and Topology; all 12 native controller IDs remain represented in component targets and eight components reported connected |
| Provider limitation | The existing desktop owned companion listener `127.0.0.1:54437`; the second service reported its bind failure while preserving all companion records |

Evidence is under `.research/system-devices-native-browser/`, including
`evidence.json`, `native-system-list.png`, `native-system-topology-initial.png`
and `native-system-topology.png`.
The observed service copy was
`.research/feature78-native/transport2/osg-gateway-service.exe`, SHA-256
`ef1559082801c4baa7576adc2dc5fa8887c307644acb99615e1c3478ce0b5315`.
This identifies the measured executable, rather than claiming later source edits
were present in that native binary. The UI uses the current development sources.

Screenshots were opened for visual inspection. The 27-node topology uses bounded
six-row sibling groups, starts at a readable 80% zoom with keyboard/pointer
panning, and fits the entire rig at 55% when Fit is selected. Labels, connection
types, current versus last-known links and type-aware counts remain distinct.
The final fixture and native runs passed together with no browser exceptions.

List now presents responsive component tiles. Running-Cockpit acceptance checks
multiple tiles per row, muted offline names with full text/control opacity,
connected-hidden emphasis, and stable base styles on retained live-update nodes.
The dev server was restarted to refresh the existing desktop after the layout
change. The actual-native List screenshot records the same 27 components as tiles.
Selecting a tile or topology node replaces the inventory with dedicated details;
Back restores the prior filters, mode and selection. Timer-driven acceptance
checks provider failure/recovery and lifecycle changes with no Refresh button.
The actual native run also records `native-system-details.png` and verifies the
inventory is hidden until Back is selected.

The final fixture evidence JSON has SHA-256
`89b5837e6df9ec95f47ed050cc78fc76bb86508b39a57398a548a996ffb8115e`;
the final native evidence JSON has SHA-256
`20357a485e47e72efda64a88dab21c3f7f030d3fbf42f35b82b191d67e800ad9`.

## Final automated checks

The final inventory/frontend suite passes **67 tests**, including compiled USB
and topology component behavior. The full native library run passes **825 tests**
with zero failures and six existing ignored qualification tests in 48.89 seconds;
its log is `.research/feature78-final/native-tests.log`. The final running-browser
fixture passes in 23.77 seconds and actual-native browser acceptance in 1.83 seconds.

Eleven focused native inventory tests cover visibility/PnP, exact publication
associations, bounded paging and configured virtual targets. A successful Missing
inspection identifies an unavailable configured target; a failed inspection leaves
its availability unknown instead of claiming confirmed disappearance.

| Production module | Measured line coverage | Evidence |
| --- | --- | --- |
| Native system inventory | 98.88% | `.research/feature78-native-missing-coverage/summary.json` |
| Native publication associations | 100% | Same summary |
| Controller visibility inventory | 100% | `.research/feature78-controller-coverage/summary.json` |
| Companion session/transport hub | 92.48% | `.research/feature78-companion-coverage/summary.json` |

Frontend inventory/projection/topology and the native modules retain their 71%
coverage gates. These software results do not advance hardware qualification.

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

Further CI fixture corrections keep supplied companion-wire tests out of OS
TLS/Keychain initialization, drive simulated feeder time from actual host renewal
messages before admitting fresh UDP telemetry, and bound each native coverage
filter with retained partial output. Ownership assertions and coverage thresholds
remain unchanged.
