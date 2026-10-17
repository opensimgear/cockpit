# Unified profile frontend retirement

The application exposes one profile authoring and activation path. Obsolete Mappings, standalone DCS and standalone integration editor components, portable schemas, client activation DTOs and their dedicated tests are removed. Game-owned provider settings and native controller/visibility discovery, inspection and restoration remain available. The gateway rejects retired command families before desktop or HTTP dispatch.

Retained output and provisioning journals have a separate cleanup-only view on Overview. The client captures exact bounded native references, including owner, generation and journal hash. It cannot construct a profile or activate outputs. Native qualification still gates both recovery and uncertain-record acknowledgement; a current native provisioning job can be explicitly cancelled. Cancel fences older unsent recovery requests and late acknowledgements. Clean status does not render a recovery card.

Validation on 2026-10-17:

- Transport: 104 tests passed, including rejected retired commands on both transport paths, independent status/cancel/Stop lanes and no replay of cancelled unsent recovery.
- Profile workspace: 69 tests passed, including five actual cleanup component/handler tests. Game launch: 11 tests passed.
- Cleanup client/controller: seven tests passed; line coverage 91.07%, branch coverage 96.86%, function coverage 100%, preserving the 71% gates.
- Current provider settings/navigation: 15 tests passed; games: 11; debug: eight; USB: 13. Companion contract/designer/renderer/library/upload/action/startup gates passed. Dedicated coverage for deleted production modules is removed with those modules; current gates retain their original thresholds.
- Both application typechecks report zero errors and zero warnings. Both frontend production builds pass.
- The existing dev page was exercised through Edge with every IPC intercepted before navigation. Two provider/cleanup browser fixtures passed: setup-revision replacement preserves a dirty form and blocks stale Save, explicit refresh/save forwards the current revision, return navigation keeps game settings, and unqualified retained records remain visible with disabled recovery/acknowledgement. Screenshots cover 1440 px and 390 px without horizontal overflow.

The browser and component fixtures do not qualify native launch, process discovery, installation, vJoy, HidHide, device writes or output restoration. No qualification flag was changed.
