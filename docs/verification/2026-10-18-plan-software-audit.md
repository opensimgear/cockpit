# Plan project software completion audit — 18 October 2026

The audit covers every Plan item in **In progress** or **In review** at the start
of this task: Cockpit #37, #79 and #83–#89. Seven were In progress; #83 and #86
were In review. No other repository had an item in those two states.

## Current software evidence

Reviewed main is `05577705b58d5f0a242e16bc79e50ffe71db0d11`.
[Checks 39152828207](https://github.com/opensimgear/cockpit/actions/runs/39152828207)
completed successfully with all 14 jobs successful, including Linux, Windows and
macOS native/controller/profile jobs, frontend checks and existing coverage gates.
[Windows release 39152828159](https://github.com/opensimgear/cockpit/actions/runs/39152828159)
and [ESP32 foundation 39152828131](https://github.com/opensimgear/cockpit/actions/runs/39152828131)
also succeeded for that exact revision. These supersede the older cancelled CI
as current software evidence; platform-conditional skipped steps are not additional
platform qualification.

The successful frontend log was inspected directly: profile workspace 88/88;
profile editor 18/18; canvas 7/7; port browser 3/3; game-provider checks 15/15;
game-launch UI checks 11/11. Editor/canvas/port-browser line coverage is
88.38% / 96.20% / 83.78%, retaining the existing 71% gates.
The workspace includes actual compiled component handlers for Default-only entry,
complete category seeding, Save/Cancel/Back, exact-profile reopening, independent
copies, offline retention, transport attributes, settings round-trip, visible
processing and captured-run isolation. Controlled IPC remains software evidence.

## Running native development Cockpit

Used the already running `src-tauri/target/debug/osg-cockpit.exe`, window title
**OpenSimGear Cockpit**, serving its UI from `http://127.0.0.1:1420/`.
Windows computer-use accessibility observations and screenshots drove the actual
desktop UI. No mock IPC or second gateway service was introduced. The observed
running executable has SHA-256
`27d4c7af37ef53bb0d95f28dbc41ebac5c772333292fda6007315b0d5f360757`;
a current CI pass does not assert it is byte-identical to the CI build.

- Starting state: seven saved profiles, 24 inventory resources, no active game,
  runtime stopped, applied revision None, Manual pinned to the existing MH-6J profile.
- Add created a blank draft with 24 resource blocks, including retained/offline
  companions/controllers and all four game integrations. It showed zero signal
  routes, transformations, virtual controllers and hide-while-active requests.
  USB/HID/integration metadata appeared on resource blocks; no transport chain
  was created. Closed-game blocks explicitly waited for matching verified context.
- Saved **Plan audit - passive profile** through native storage at revision 1.
  Back returned to the eight-profile list. Reopening that exact row retained
  revision 1 and all 24 blocks.
- Renamed the draft to **Plan audit - unsaved edit** and used Back. The native
  unsaved-changes dialog offered Keep editing, Discard and continue, and Save and
  continue. Save and continue committed revision 2 and returned to the list.
- Added another blank draft and Cancel creation returned to the same eight-profile
  list without creating another saved entry.
- Throughout these checks, the existing Manual pin, stopped state and applied
  revision None stayed unchanged.
- Native Games → DCS World exposed DCS-BIOS 0.11.7, MH-6J catalogue, connection,
  installation/repair and Automatic rule settings in game details. Provider setup
  was Ready for integration while game context independently showed Waiting for
  an active profile / No verified matching process, and telemetry waited for data.
  No standalone DCS-BIOS navigation entry was present.

The passive revision-2 test profile is retained for inspection. No Apply, game
launch, automatic-mode enablement, output or hiding activation was needed.
UI Automation cached offscreen row bounds after scrolling; screenshot-relative
clicking opened the intended visible row, and the resulting title/revision was
verified. This is an automation limitation, not a product routing failure.

## Story decisions and remaining acceptance

| Story | Decision | Remaining work |
| --- | --- | --- |
| #83 P3 | Done; close software milestone | Actual DCS response, combined rig and release qualification stay in #89/#79 and existing qualification tickets |
| #86 P6 | Done; close software milestone | P7 live graph inspection and P9 combined runtime acceptance remain separate |
| #37 Companion feature | Keep In progress | Real Android/iOS/second-PC lifecycle, signed install/update/trust, load/latency and release matrix qualification in #55–#62/#66–#68 |
| #79 Unified profiles | Keep In progress | Remaining P4/P5/P7/P8/P9 acceptance; P3/P6 completion is not parent completion |
| #84 P4 | Keep In progress | Ordinary saved-profile Default/custom/Apply/Pause/recovery acceptance and applicable selected-rig loss/restoration; preserve previous bounded same-slot handoff and simulated-service crash evidence |
| #85 P5 | Keep In progress | Actual DCS CommonData Text AND hysteresis Indicator, live freshness/loss/isolation and required applicable physical actions; OSG hardware was explicitly deferred |
| #87 P7 | Keep In progress | Complete native live/stale graph/learning diagnostics, transformations after reopen, and editing isolation against an applied run |
| #88 P8 | Keep In progress | Ordinary saved-rule Automatic/external launch/exit, Manual/Pause/Stop inhibition and native launch acceptance on the selected rig |
| #89 P9 | Keep In progress | Combined Windows/VKB/pedals/vJoy/HidHide/DCS/browser-companion run, lifecycle/loss/restoration and applicable restart/recovery acceptance |

P3 closure is supported by [its provider/context verification](2026-10-17-unified-profiles-p3.md),
[observation-order regression](2026-10-18-game-context-observation-order.md),
the current all-platform CI and the actual game-details observation above.
P6 closure also uses [its complete authoring/navigation verification](2026-10-17-unified-profiles-p6.md)
and [keyboard/browser acceptance](2026-10-17-profile-path-activity.md).
The narrower native checks here supplement those automated suites; they do not
claim to repeat every fixture or prove hardware behavior.

Old copied notes that a qualification cycle was intentionally active are historical:
the actual desktop observed here is stopped with no active game. This audit does
not assert restoration of every historical temporary game file. Existing
qualification flags remain unchanged. Actual game input, physical output,
signed/mobile distribution and omitted recovery cases are not marked passed.
