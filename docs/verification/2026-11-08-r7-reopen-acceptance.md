# R7 background updates and reopen

The Windows desktop and Edge browser-dev ran a real pending launch on
Overview with Profiles closed. Discovery and Apply registered only the
fixture's private Steam library entry. The selected launcher wrote its
arguments and exited. Its distinct registered runtime stayed absent, so the
native launch worker published its real waiting state. The fixture used an
output-free profile and checked both executable paths before launch.

The native child window was hidden and revealed with its original Windows
handle. The browser fixture dispatched document visibility changes. Each
presenter displayed the current waiting message on reveal. An independent
gateway request cancelled the exact operation while the presenter was
hidden. The native stream reported a cancelled, inactive outcome, the
original launch settled, and the presenter retired. No profile owner
remained. The trace found zero legacy activation discovery or status reads.

The initial fixture used retired discovery commands and stopped before
launch. The final fixture waits on the retained discovery stream and uses
the current review and Apply commands. The native binaries were built from
`d79b1b94`, whose tracked native source matches the fixture base
`bcf90720`. The 28 focused tests passed. Presenter and native window
coverage were 93.88–97.44% lines, 85.71–86.92% branches and 89.47–95.24%
functions. All 1,715 frontend tests passed.

## Scope verified

| Surface | Running evidence and boundaries |
| --- | --- |
| App, workspace and Devices | Shell and Devices reopen on the connected replacement after both were closed. The Windows desktop covers the shared idle baseline, while native Linux and browser-dev cover the simulator transport. Physical Windows USB gateway transitions remain in #199. [Workspace](../reactive/read-model-sources.md) |
| Controllers, system USB and visibility | The selected input and metadata readers retire while hidden and reopen on the current baseline. Visibility preference commits and CAN input have separate fixtures. Real controller display cadence, calibrated endpoints and physical USB/CAN acceptance remain in #199. [Controller preview](2026-11-08-controller-preview-visibility.md) · [USB details](2026-11-08-usb-details-visibility.md) · [Visibility](2026-11-08-controller-visibility-reader-lifetime.md) · [Source matrix](../reactive/read-model-sources.md) |
| Profiles and Debug | Library, draft, applied runtime, port inventory, automatic selection, inspection/topology, virtual preview and fault recovery follow current commits. The previous learning panel was removed before R7. Both gateways reopen on the exact cancelled capture, and the editor uses its port catalogue. [Profile and Debug evidence](../reactive/remaining-acceptance.md#r7-ui-consistency) |
| Games and telemetry | Discovery, rules, installation edits, artwork and Debug telemetry reopen on current prepared data. The authored DCS run keeps acquisition separate from the selected reader. Real simulator confirmation remains in #199. [Games sources](../reactive/read-model-sources.md) · [Telemetry parity](2026-11-07-telemetry-reopen-parity.md) |
| Companions and designer | Roster, pairing, listener, library, assignments and firmware release readers follow current commits. Expanded Debug reads the same retained USB flash status as MCP. Real worker failure and withdrawal have authored evidence. The per-companion update wizard tracks its own exact operation. Physical flash and native mobile shell acceptance remain in #199. [Companion sources](../reactive/read-model-sources.md) · [Release metadata](2026-11-07-companion-release-observation.md) |
| Settings, MCP, Activity and notifications | Both clients retire scoped readers and display current settings and Activity after background commits. Browser-dev displays the desktop-only boundaries for native Backups and MCP administration. Global save notices expire on reveal even if a renderer delays their callback. [Settings lifetimes](2026-11-08-settings-reader-lifetimes.md) · [Activity and MCP sources](../reactive/read-model-sources.md) · [Notifications](2026-11-08-notification-deadline-reveal.md) |

The [idle matrix](2026-11-08-idle-transport-matrix.md) recorded zero gateway
requests across all eight top-level views on both transports for 60 seconds.
The integrated acceptance matrix remains in #198.

Prepare a fresh run directory with
[the launcher fixture](../../tests/ui/profile-progress-launch-fixture.ps1).
Set the isolated Cockpit or Vite process's `ProgramFiles(x86)` to that
directory's `programs` subdirectory, and give both the launcher and
[the test](../../tests/ui/profile-progress-live-reopen.mjs) the same
`OSG_SQLITE_DATABASE` at `cockpit.sqlite` inside the run directory. Set
`OSG_PROFILE_PROGRESS_LIVE_RUN` to the run directory. Set the native
`OSG_PROFILE_PROGRESS_LIVE_APP_PID` to the isolated Cockpit PID, or use
`OSG_PROFILE_PROGRESS_LIVE_BROWSER_URL` for browser-dev.
