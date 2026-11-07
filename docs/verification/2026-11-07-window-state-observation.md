# Live window persistence diagnostics

The window persistence owner now publishes `settings.window-state` when a
restore, capture, save, or flush diagnostic changes. The payload contains a
bounded error string or `null`. The saved geometry is a separate next-launch
value. A successful write clears the save and restore diagnostics while a
separate capture failure remains until a valid capture. Duplicate status
leaves UI and MCP readers quiet.

The Windows dev Cockpit used a disposable SQLite database. The fixture held a
real `BEGIN IMMEDIATE` transaction and resized the native window without
activating it. General showed the failed save while open. Switching tabs
retired the exact reader. Reopening General while the lock was held received
the retained error on a fresh baseline. Releasing the lock let the existing
writer retry save the new geometry and clear the diagnostic without another
status request. The trace counted one desktop preference read on each of the
two visible opens. The first test run exposed the existing toast-only
presentation. The final pane uses an inline status so the clear is visible.

The native unit test independently held an owned commit, introduced a foreign
geometry revision, and checked that both UI and MCP readers received the
conflict and subsequent recovery. The stale write left the foreign revision
intact. Browser-dev passed the existing General Settings reopen test.

The native dev executable SHA-256 was
`743163e125ec49891929b946db7e4b216feebf6d94da3103caadc52919ff0ffb`.
The isolated frontend passed all 1,700 tests, Svelte checking, the UI preference
coverage gate and the production frontend build.

The full Windows suite passed 2,419 tests with 18 default ignores before the
lock-recovery cleanup. Both pinned DCS archive tests passed separately. On
the final native sources, the three window writer tests and 15 MCP tests
passed. Production line coverage reached 98.61% for the writer and 100% for
MCP URI admission. LLVM reported 100% function coverage for both modules.
The instrumented run emitted no branch data.

To repeat the running check, launch a disposable Windows dev Cockpit with
`OSG_SQLITE_DATABASE` pointing to `cockpit.sqlite` under a private directory
named `window-state-qualification-<number>`. Set `OSG_WINDOW_STATE_NATIVE=1`,
`OSG_WINDOW_STATE_ROOT`, `OSG_WINDOW_STATE_PID`, `OSG_WINDOW_STATE_BINARY`, and
`OSG_NATIVE_CDP_URL` for `node --test tests/ui/window-state-observation-native.mjs`.
