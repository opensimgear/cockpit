# X-Plane passive inspection demand

Explicit inspection demand now schedules its own lease deadline. Observation
demand lasts until its observation session closes. Closing that session wakes
the vehicle worker even when X-Plane is silent. Work deferred during a detached
read remains due when the reader returns.

The existing X-Plane owner applies the combined channel demand. Capture reads,
process qualification, and output authority use their existing contracts.
The vehicle worker's general process verification is still needed for X-Plane.
Actual simulator and Windows monitor consent remain in #199.

The focused native tests verify exact deadline selection, renewal and closure
of a quiet observation. The frontend telemetry suite passes all 14 tests.
The Windows dev build passed, with executable SHA-256
`b1d86afd0ab8d71a4b029a32ce0217263cb02fe0dcbf8659425e74bd5c3c8792`.
The running Cockpit passed passive inspector and workspace subscription
checks. The inspector used temporary unlaunched catalogue entries, removed
after the check. The timing audit accounts for all 247 pinned sites.

The full native suite passed 2,343 tests with 17 ignored. All seven coverage
gates passed. Inspection demand measured 100.00%, vehicle views 99.21%,
vehicle polling 93.86%, tracker 98.08%, game context 95.27%, observations
94.91%, and the telemetry API 93.37%.
