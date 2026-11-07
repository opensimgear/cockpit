# Watched DCS profile scheduling

Supported Windows DCS profiles can park on native producer notifications and
their existing deadlines. A live BIOS transport and watch are required. If the
profile selects ownship physics, that producer must have its own live watch.
Both the process/context worker and independent catalogue worker must be
started. The host schedules the earliest proof refresh or expiry. BIOS and
ownship expiry are published by their respective producers.

Absent or closed watches, BIOS transport retry, deferred inventory and
context recovery retain bounded service. Unsupported adapter combinations
remain outside this eligibility gate.

The host fixture uses a real decoded BIOS watch. It covers the scheduled path
and rejects absent catalogue or ownship watches and pending BIOS retry.
Actual simulator routing and timing acceptance remain in #199.

## Verification

- All 2,357 native tests passed with 18 existing ignores. Host scheduling
  and source wakes reached 77.55% and 78.16% production line coverage.
- All 14 telemetry frontend tests passed. The timing audit verified 247 sites.
- The Windows dev build and three passive Cockpit checks passed. The dev
  catalogue had no registered DCS installation for the active fixture.
- Dev executable SHA-256 was
  `ee3737b634ba91eadee6079b597fd716039c2100e0e06d51e6404530367b38de`.

## Direct DCS command scheduling

The active host can also park with a simulator-command sink when the exact
applied game qualifies as watched DCS. The bounded command worker already
wakes the host on completion or failure. Command expiry, HID publication
proof checks and the deadline for retrying a pending withdrawal still apply.
An unwatched source or another integration retains bounded service.

- The scheduler regression verifies the idle service decision with a DCS
  command sink, then verifies fallback after watch retirement or a different
  integration. The native suite passed 2,373 tests with 18 existing ignores,
  including command worker FIFO and wake coverage and exact UDP submission.
- Host scheduling reached 75.95% lines and 75.00% functions. The coverage run
  did not instrument branches.
- The rebuilt Windows dev Cockpit passed the authored BIOS and ownship
  fixture, including independent expiry, stalled inspection and process exit.
  This run exercised telemetry only. Actual DCS command effects and physical
  input latency remain in #199.
- Dev executable SHA-256 was
  `bb29205106b1655f5d3ab87c385ca48fe706993c9e1379005815fbbe2c0684f6`.
