# Applied-profile validation wakes

The native workers for applied-profile game identity and DCS catalogue validation wake the serialized profile host after queuing a result. A launch-to-host handoff redirects an in-flight worker's wake target. Notifications carry no proof or output authority. The consumer still checks result, identity and proof age before publishing game data.

The process proof retains its 25 ms refresh and 100 ms expiry. The DCS catalogue proof retains its 125 ms refresh and 250 ms expiry. This change covers completion delivery. Active game service still schedules proof refresh and expiry on its existing bounded cadence.

## Verification

- Native suite passed with 2,350 tests and 18 existing ignores. The validation module covered 68 of 69 production lines (98.55%).
- All 14 telemetry frontend tests passed. The audit verified all 247 pinned timing-site dispositions.
- The Windows dev build and two running Cockpit checks passed. The checks covered passive telemetry subscription cleanup and the shared workspace baseline.
- Dev executable SHA-256 was bfc0f71a23b850aff4901d0d4b26c123ea7c60fc99ccfe91d43f0d3e8c1f45ea.
- Active simulator and physical controller latency qualification are tracked in #199.
