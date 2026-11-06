# Watched DCS profile scheduling

Supported Windows DCS profiles can park on native producer notifications and
their existing deadlines. A live BIOS transport and watch are required. If the
profile selects ownship physics, that producer must have its own live watch.
Both the process/context worker and independent catalogue worker must be
started. The host schedules the earliest proof refresh or expiry. BIOS and
ownship expiry are published by their respective producers.

Absent or closed watches, BIOS transport retry, deferred inventory and
context recovery retain bounded service. Simulator commands, vehicle variants
and other unsupported adapter combinations are outside this eligibility gate.

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
