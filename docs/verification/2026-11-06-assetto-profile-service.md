# Watched Assetto profile scheduling

Supported Windows Assetto profiles now use retained telemetry and resource
notifications in steady state. The active host schedules the retained
capture's exact expiry and the existing process proof deadlines. A capture
watch qualifies service only after the validation worker starts and the
prepared game and applied owner refer to the same game.

The 5 ms fallback remains for missing or closed watches, deferred inventory,
recovery, and profiles with unsupported adapters or vehicle variants. This
change does not alter the shared-memory acquisition owner or its cadence.
The process monitor consent step and physical simulator acceptance remain
tracked in #199.

## Verification

- All 2,356 native tests passed with 18 existing ignores. Host scheduling and
  source wake coverage reached 77.55% and 73.75%. The host fixture exercises
  watch-backed scheduling, deferred inventory and watch closure.
- All 14 telemetry frontend tests passed. The audit verified 247 timing sites.
- The Windows dev build and three passive running Cockpit checks passed.
  They covered telemetry subscription teardown, the shared workspace baseline
  and attached controller preview backlog recovery.
- Active Assetto routing remains unverified in the running app pending native
  Windows process-monitor consent. Dev executable SHA-256 was
  `50708749cde398238cc1ed870bc3294229c85c953a63956626b36b4e9c2c369b`.
