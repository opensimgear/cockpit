# X-Plane vehicle process evidence

Watched X-Plane vehicle readers use the process monitor already held by their
native capture owner. A weak reference exposes monitor health without keeping
that owner alive. The journal watch wakes on terminal loss, even before a live
telemetry packet arrives. A failed monitor or closed journal withdraws cached
telemetry and fences detached results.

Quiet monitored readers no longer schedule the vehicle worker's 100 ms process
verification. Capture, terminal loss, and inspection demand deadlines retain
their independent wakes. Readers without retained process evidence still use
the existing checks. The separate game-context process pass and applied-profile
validation are outside this vehicle worker change.

Actual X-Plane qualification and Windows process-monitor consent remain in #199.

- A native X-Plane owner fixture verified that quiet admitted readers have no
  periodic process deadline and that monitor loss withdraws telemetry and
  wakes the worker with no simulator packets.
- The Windows dev build passed, with executable SHA-256
  `e00cce4cc313f36363c767c457f31af15f514a4724974bfd919aa84bb2dcf544`.
  The running Cockpit passed the passive inspector and workspace subscription
  checks. The temporary unlaunched catalogue entries were removed afterward.
- All 14 frontend telemetry tests passed. The timing audit accounts for all
  247 pinned sites.
- The native suite passed 2,344 tests with 17 ignored. All six coverage gates
  passed. The X-Plane owner and client measured 95.97% and 90.40%. Vehicle
  sources, tracker, polling and views measured 87.16%, 98.08%, 93.86% and 99.21%.
