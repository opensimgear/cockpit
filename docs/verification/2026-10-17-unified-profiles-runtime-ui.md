# Unified profile runtime controls — 2026-10-17

The Profiles runtime card consumes the actual native P4 session status and
explicit Apply/Pause/Stop/recover commands. It shows immutable applied revision
and run identity independently from the currently edited revision, and reports
native degradation/recovery errors without inventing successful ownership.

| Check | Result |
| --- | --- |
| Workspace/routing/runtime actual Svelte SSR and handler tests | 26 passed |
| Runtime client | 5 passed; 100% lines, branches and functions |
| Runtime controller | 6 passed; 100% lines, 89.47% branches, 100% functions |
| Main and browser Svelte checks | 0 errors, 0 warnings |
| Browser and desktop UI production build | Passed; desktop 237 modules |

The controller's independent 71% line/branch/function gate passes six tests at
100% lines, 89.47% branches and 100% functions. Tests exercise exact saved-revision
admission, captured Apply arguments, interruption and late replies, failed effect
requests without retry, read-only status recovery, persistent pause clearing and
disposal without output effects.

Actual Svelte SSR and real-handler tests exercise disabled unsaved/read-only
Apply, the exact saved identity/revision and explicit game-context request,
applied revision isolation, a retained ineligible game choice, enabled Stop
during delayed Apply, persisted pause clearing, recovery results and native
degradation/fault rendering. The real polling lifecycle only issues status reads,
suspends while hidden or retained behind settings, and removes its listeners and
timer without stopping a native run. Panel response-admission tests reject older
selection metadata, malformed native status and responses arriving after disposal
without refreshing or replacing the editor/history.

These checks use actual Svelte code with SSR and handler/lifecycle instrumentation.
They do not establish native interactive focus, browser pointer capture, real
driver/device output, live-port learning or simulator qualification. Runtime
effects remain subject to the native coordinator's independent authority checks.
