# Windows game process boundary verification

These measurements cover the earlier 500 ms fallback. The [approved Settings
opt-in and one-second fallback](2026-11-07-game-process-discovery-preference.md)
supersede that configuration.

The Windows catalogue watcher subscribes to process-start and process-stop
events for registered executable names. These notifications wake an exact
inventory lookup. A name alone cannot establish the selected game identity.
When Windows denies the subscription, Cockpit currently checks inventory
on a 500 ms fallback. Automatic activation then requires two matching
observations of the executable path and creation epoch.

From the ordinary Windows session, filtered temporary subscriptions to both
`Win32_ProcessStartTrace` and `Win32_ProcessStopTrace` returned access denied.
Windows permissions were left as configured. The current
[acquisition ledger](../reactive/acquisition.md) describes the event and
fallback paths. The decision on retaining fallback sampling remains open.

The disposable native Cockpit at `e9e7968e` ran
`tests/ui/game-process-boundary-native.mjs`. The isolated dev frontend's
main view, header, shared Games model and native transport matched main.
The fixture registered one
private executable and launched a different copy with the same filename.
Neither the shared Games read model nor the Active game header reported the
registered game for that process. Eight external launches of the registered
executable and eight exits produced exactly 16 matching transitions in
each surface. The trace observed zero `game_get_state` or
`game_read_model_page` requests after the positive control.

| Local test interval | Observed Active game header range |
| --- | ---: |
| Child launch request to detection | 744.65–1078.69 ms |
| Child termination request to withdrawal | 217.27–346.34 ms |

The intervals include child launch or termination, detection, projection
and delivery to the actual WebView. The zero query count covers the WebView.
Native fallback inventory remains active. The run qualifies authored process
identity and UI delivery under the denied-event path. Live Windows event
consent, continuity and simulator effects remain in
[#199](https://github.com/opensimgear/cockpit/issues/199).

The 32 game read-model tests passed locally. Coverage was 95.63% lines,
87.82% branches and 85.05% functions. Tested Cockpit binary SHA-256 was
`9b69eea9eb3edeaa2b4356ef5d29a01e335993f986737e45d52580096ffd8881`.

## Watcher teardown and replacement

The activation owner now receives a completion wake after the native watcher
releases its COM subscription. Regressions cover the order of the initial
continuity-loss wake and completion wake, plus the bootstrap-failure path
where the watcher never establishes continuity.

The running dev Cockpit repeated the authored process fixture with the
teardown change. Both surfaces delivered 16 matching start and exit
transitions, the same-name decoy was ignored, and the trace contained zero
repeat gateway reads. The header observed starts in 778.79–1077.40 ms and
exits in 253.59–345.44 ms. These timings cover the fallback path from the
child launch or termination request to the WebView. The tested binary
SHA-256 was `a4a27e62a89ef8053bee92f6d0be7c2c2793bb22ddae961dd709ea0743e6d29c`.

The isolated Windows host suite passed 2,414 tests with 18 default ignores.
Both pinned DCS archive tests passed when run separately, as did the 27
frontend game-launch tests. The changed native module reached 89.19%
production line coverage. LLVM reported 89.66% function coverage for the
module including tests. This toolchain run did not emit branch coverage.

## Native and browser-dev delivery with executable replacement

The running Windows WebView and Edge browser-dev each delivered 20 matching
header and shared read-model transitions. Each ignored the same-name decoy,
observed eight start/exit cycles, then withdrew the old identity after a
`game_save` replaced the registered executable path while that old process
was still running. Exiting the retired process left the new registration quiet.
Starting and stopping the replacement produced one transition each. Each UI
trace contained zero `game_get_state` or `game_read_model_page` requests during
the process changes. The catalogue commit is a separate measurement because a
committed catalogue change can require an immutable page load.

| Transport | Start to header | Exit to header | Executable save to withdrawal |
| --- | ---: | ---: | ---: |
| Windows WebView | 560.22–856.63 ms | 233.43–372.65 ms | 28.35 ms |
| Edge browser-dev | 777.97–1042.89 ms | 247.92–338.10 ms | 42.46 ms |

The intervals include the authored process action or save request, detection,
projection, and delivery. Windows process-event consent and the fallback
disposition remain separate gates. The WebView executable SHA-256 was
`743163e125ec49891929b946db7e4b216feebf6d94da3103caadc52919ff0ffb`.
The browser-dev gateway SHA-256 was
`f17639511e24d5d84f68d04abcff16766a6a77ce4e20d7f3fa6042c1bc4c0c98`.
Cargo linked the new gateway under `target/debug/deps`. Access to the old
top-level executable prevented Cargo from copying over it, so the fixture
launched the newly linked executable directly.

All 32 game read-model tests and 27 game launch tests passed. The game
read-model coverage gate measured 94.87% lines, 88.08% branches, and 85.05%
functions. Both transports also passed the full navigation and observation trace.

To repeat on a disposable running Windows dev Cockpit, set
`OSG_NATIVE_CDP_URL` to the main WebView's CDP endpoint and set
`OSG_GAME_PROCESS_BOUNDARY_NATIVE=1`. For browser-dev, set
`OSG_GAME_PROCESS_BOUNDARY_BROWSER_URL` to the local frontend URL backed by the
Windows gateway service. Run `node --test tests/ui/game-process-boundary-native.mjs`.
The fixture uses its authored children and removes its registered game afterward.

## Alternative Windows event sources

A capability probe from the same ordinary Windows account called
`StartTraceW` with real-time mode, no log file and a small buffer pool.
It returned Windows error 5 (`ERROR_ACCESS_DENIED`) before the process provider
could be enabled. The token was neither an effective administrator nor a
member of Performance Log Users. Microsoft documents the trace-controller
privilege boundary. [StartTraceW return codes](https://learn.microsoft.com/en-us/windows/win32/api/evntrace/nf-evntrace-starttracew#return-value)

The installed `Microsoft-Windows-Kernel-Process` provider has an Analytical
channel, which was disabled on this host. The Event Log subscription API
accepts Admin and Operational channels. Reading the local process creation
and termination audit policy returned error 1314, so audit-log availability
was unverified. No privilege, audit or channel settings were changed.
[Windows Event Log subscriptions](https://learn.microsoft.com/en-us/windows/win32/wes/subscribing-to-events)

A first `logman` probe hit its system-drive space requirement. The memory-only
native call isolated the permission result. The existing scoped WMI helper
remains the Windows event path that requires operator acceptance. The
decision on the fallback was later recorded in #189 and implemented as a
[Settings opt-in with one shared one-second check](2026-11-07-game-process-discovery-preference.md).
