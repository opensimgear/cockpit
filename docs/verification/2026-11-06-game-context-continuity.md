# Game context inventory continuity

The activation worker publishes the live process subscription scope for the
library revision it actually watches. The context worker retains a successful
inventory only within that scope and the same process event revision. Process
events, scope retirement, runtime selection changes, and failed inventory reads
require a fresh batch. Explicit context refreshes also read a fresh inventory.

With a live scope and a current inventory, the worker parks until its setup or
vehicle retry deadline or a native wake. Vehicle identity changes wake context
without invalidating the inventory. An unavailable or incomplete native scope
and an unsuccessful inventory keep the existing 500 ms fallback. Maintenance
still verifies exact liveness before publishing a selected game. Setup fallback
checks run on their two-second deadline.

The Windows dev build passed with executable SHA-256
`65be47f1d1cfdf437e201bdc7d5653094d8a7b88b9130c4f8b166b3a9a89b932`.
The rebuilt Cockpit passed the passive inspector and workspace subscription
checks. The two temporary unlaunched catalogue entries were removed. All 14
frontend telemetry tests passed. Live process-event consent and actual simulator
qualification remain in #199.

The native suite passed 2,349 tests with 18 ignored. All six coverage gates
passed. Discovery, context, signals, vehicle tracker, observation and polling
measured 88.24%, 96.14%, 98.47%, 98.15%, 98.45% and 94.78%. The timing audit
accounts for all 247 pinned sites.
