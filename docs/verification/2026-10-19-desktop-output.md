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
Native application sources match `d711f14`; the final test harness is `8220ce8`.
The production gate remains false. These results establish software behavior
and native UI authoring, without qualifying physical controllers, game input
consumption, installed deployment or capture-to-submission latency.

The running Cockpit also passed the two-source [native mouse ownership check](2026-10-19-desktop-ownership-native.json).
Two real authenticated WSS peers share MouseLeft without duplicate edges. A's
release and acknowledged protocol session close preserve B's hold; reopening A
requires a complete neutral baseline and uses a new native generation. B's full
held producer stays unchanged. After Stop, replacing only A's publication and
explicitly applying revision 2 works again. The receiver observed exactly three
down/up pairs. Cleanup proved an empty ledger and zero workers, restored local
preferences and removed the test profile. Reproduce with both
`OSG_DESKTOP_OWNERSHIP_NATIVE=1` and `OSG_DESKTOP_NATIVE_INJECT=1`, then run
`pnpm desktop:ownership:native`. This covers protocol session loss rather than
abrupt socket termination or physical unplug.

The updated [companion acceptance](2026-10-19-desktop-companion-native.json) and
[actual ACR launch/scope acceptance](2026-10-19-desktop-game-native.json) each
passed against the same development application. Both now require `waiting`
after focus returns while a control remains held. The companion run also
verified fresh-neutral rearming, upstream Macro cancellation and lease cleanup.
The ACR run verifies native submission and scope, with game consumption still
unmeasured.

Focus-loss mouse delivery has a specific observation limit. Earlier runs failed
an assertion that the second receiver must receive `WM_LBUTTONUP`; replacing
input-queue attachment with cooperative receiver focus did not produce that
message. The final test records all four keyboard releases in the second
receiver and fresh OS left-button samples changing from down in the selected
receiver to up in the second receiver. Samples require the exact foreground
window, eligible desktop, unswapped buttons and newer local sequence/time;
unavailable queries expose no button state. Ordinary press/release, rearmed
input and lease-loss checks still require receiver mouse messages. The focus-loss
mouse-up recipient remains unidentified. Windows documents [cursor/capture mouse
routing](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-lbuttonup)
and the [state-query limits](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getasynckeystate).
These example-only queries never provide production input ownership authority.

The companion wrapper now independently stops and proves clean ownership before
restoring preferences or closing its receivers, including after child failure.
Unproved cleanup retains the targets and recovery settings. The final native
runs all completed cleanup successfully with no worker remaining and desktop
output disabled.

The UI fix at `d711f14` also passed the native test and packaging steps in
[Windows release run 39210982640](https://github.com/opensimgear/cockpit/actions/runs/39210982640)
and the [ESP32 build](https://github.com/opensimgear/cockpit/actions/runs/39210982722).

The preceding finalized feature snapshot `8c782695dafd948621ca54b40ccd8bc3b263dbc5`
passed all 14 jobs in [CI run 39165938582](https://github.com/opensimgear/cockpit/actions/runs/39165938582)
and the [ESP32 build](https://github.com/opensimgear/cockpit/actions/runs/39165938634).
The [Windows package run](https://github.com/opensimgear/cockpit/actions/runs/39165707490)
also passed at source `8169da1`. These results precede the fixes in this record.
