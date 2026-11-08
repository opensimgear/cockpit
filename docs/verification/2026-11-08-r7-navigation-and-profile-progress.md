# R7 navigation and profile progress

The running Windows Cockpit and Edge browser-dev passed nine navigation,
profile, progress and workspace tests against the application source from
`2446a7e1`. Each transport used a private database. The profile fixture
waits for the main portal and native IPC before importing its probes. A
cold WebView initially rejected those imports before the portal was ready.
An intermediate extension also failed before issuing Stop because its
operation ID helper lacked an import. The final runs passed on both paths.

| Surface | Verified behavior |
| --- | --- |
| Navigation and Debug | Two cycles through Devices, Profiles, Games, Companions, Debug and Activity. Each accepted reader received one baseline. Exiting scoped views retired their readers. Companions details hid and reopened on one shared status baseline. |
| Devices settings | Both transports retired the relevant readers while hidden. Reopening used one inventory read and one visibility read. One visibility commit caused one read. A background desktop output preference change reached the visible switch, and a second write while closed appeared on reopen. |
| Companion listener | Each transport reopened on the listener enable committed while Settings was hidden. The hidden view issued zero reads. Reopening issued one status read. The test used a private loopback port. |
| Activity and Backups | Both transports followed the Activity retention stream. Native Backups reopened on its current catalogue with zero fallback reads, then recovered the queued restore after broker shutdown with one status read. Browser-dev displayed the desktop Backups boundary without opening a reader. |
| Profiles library and editor | Both transports followed an independent rename while visible and a later rename while closed. A dirty local draft survived a competing commit. Two actual editor saves persisted the debug controller range and the active revision changed on reapply. |
| Runtime and activation progress | The real activation stream on each transport delivered success for manual activation and saved changes under distinct operation IDs. A Stop issued on Overview through a separate gateway client delivered the exact requested operation ID and the inactive outcome. The runtime withdrew its owner, progress expired, and the reopened Profiles page offered Activate. Neither transport issued profile status, list, automatic status or activation status refresh requests in this fixture. |
| Progress presentation | Native presentation probes verified the system preference's Cockpit window fallback, exact Cancel intent, noncancellable cleanup, stale operation rejection and four positions without taking focus. Browser-dev presentation used three authored observation frames to check background delivery and terminal clearing on reveal and reload. |
| Native workspace | Two consumers shared the immutable baseline. Removing one retained the current cache. The passive probe issued no frontend discovery or state queries during its 6.5 second observation. The earlier browser-dev simulation covers gateway arrival, replacement while closed and removal. [Workspace source and fixture](../reactive/read-model-sources.md) |

The fixtures used an output-free debug controller profile. Their activation
time includes authoring and admission and does not measure controller input
latency. The native notification probes used a visible window and the browser
presenter used synthetic document visibility. Live profile progress in the
editor fixture came from the unmodified native publisher on both transports.

The remaining #196 gate still needs a complete map of nested consumers.
In particular, these tests leave live learning and topology changes across
reopen, automatic selection and recovery presentation, and native workspace
arrival/replacement outside this run. Earlier companion release tests cover
catalogue metadata separately from the board lifecycle. [Release verification](2026-11-07-companion-release-observation.md)

## Local checks and evidence

The isolated frontend passed 65 tests across `profile-editor:coverage`,
`profile-runtime:coverage` and `profile-runtime-controller:coverage`. Their
lowest line, branch and function results were 96.67%, 91.37% and 95.24%.
This change only extends the running acceptance fixture and the evidence.

Raw logs are under `.research/reactive/r5-read-model-qualification/` in
`navigation-{native,browser}.log` and
`navigation-supplement-{native,browser}-verified.log`.

Native executable SHA-256
`0b550fa4c7acd1c196368c95d0d8335083f875a45f02feed6de33e7323285954`.
Browser-dev gateway SHA-256
`2ae18bd682728d1a07cbb9271bcebe5f2261e1b04623fc0e8965acac3cfb310d`.
