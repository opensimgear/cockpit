# Windows keyboard and mouse development verification — 19 October 2026

This supplements the [18 October record](2026-10-18-desktop-output.md) for
feature [#97](https://github.com/opensimgear/cockpit/issues/97). Production
desktop activation remains disabled; the remaining release matrix still applies.

The running Windows development Cockpit passed its native authoring test after
the key picker focus fix. Clicking Capture waits for the actual native Pause,
focuses the rendered picker, and captures the first `ArrowRight` as HID usage 79
without another click or explicit test focus call. Output remains paused after
capture. Create, save, reopen, export, import and recording preview also passed.
Cleanup deleted the three profiles created by this run and restored the prior
Pause value. Local evidence is in `.research/desktop-output-native-authoring/`.

The native desktop inspector now reports `waiting` when the application is
focused but some actions still require fresh neutral input. Other armed actions
can continue. A mixed-source regression verifies this behavior and verifies
that Stop releases the held action. The full mapping-host library suite passed
103 tests, the desktop frontend suite passed 14 tests, and both Svelte projects
reported zero errors and warnings.

The running development executable was built with explicit
`OSG_DEV_DESKTOP_INPUT=1`; its SHA-256 is
`c6d3b0b0affd5981d233915938f93e9280253ff918fe5104e2edf50582d85497`.
The production gate remains false. These results establish software behavior
and native UI authoring, without qualifying physical controllers, game input
consumption, installed deployment or capture-to-submission latency.

The preceding finalized feature snapshot `8c782695dafd948621ca54b40ccd8bc3b263dbc5`
passed all 14 jobs in [CI run 39165938582](https://github.com/opensimgear/cockpit/actions/runs/39165938582)
and the [ESP32 build](https://github.com/opensimgear/cockpit/actions/runs/39165938634).
The [Windows package run](https://github.com/opensimgear/cockpit/actions/runs/39165707490)
also passed at source `8169da1`. These results precede the fixes in this record.
