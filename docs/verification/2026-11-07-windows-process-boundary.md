# Windows game process boundary verification

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

To repeat on a disposable running Windows dev Cockpit, set
`OSG_NATIVE_CDP_URL` to the main WebView's CDP endpoint and set
`OSG_GAME_PROCESS_BOUNDARY_NATIVE=1`. Run
`node --test tests/ui/game-process-boundary-native.mjs`. The fixture uses only
its authored children and removes its registered game afterward.
