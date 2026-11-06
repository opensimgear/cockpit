# Game context process inventory

The existing Windows inventory subscription wakes both profile activation and
game context when process lifecycle events arrive, after subscription bootstrap,
and when continuity is withdrawn. The game context wake is independent of the
library revision. A process event does not force a provider setup refresh.

Each context pass validates eligible installations before batching their runtime
paths into a single native process inventory. Results remain ordered per game.
An invalid path affects its own result, and a selected process still passes the
existing exact liveness check before publication. Telemetry selection keeps the
existing distinction between the Assetto Corsa launcher and the runtime.

The 500 ms context pass remains for scheduled maintenance and environments
without a process subscription. The activation and context workers still
qualify their own process results. Live simulator and Windows notification
consent acceptance remain in #199.

The Windows batch regression uses a real process to check ordered selection
and isolate an invalid path. Worker regressions cover the independent process
wake and notification of both consumers on continuity loss.

The Windows dev build passed with executable SHA-256
`17f4102a6768db5776caa31bb7d129b2c080a1871c926486cd86daf34f3f5b7b`.
The rebuilt Cockpit passed passive telemetry inspector and workspace
subscription checks. Temporary unlaunched catalogue entries were removed.
All 14 frontend telemetry tests passed. The timing audit accounts for all
247 pinned sites.

The complete native suite passed 2,348 tests with 18 ignored. The four
coverage gates passed. Discovery measured 88.33%, game context 96.00%,
read-model signals 99.13%, and activation 98.63%.
Staged native records match the candidate after line-ending normalization and
removal of one trailing blank line in the discovery test file.
