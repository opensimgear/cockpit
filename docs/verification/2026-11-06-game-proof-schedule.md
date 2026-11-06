# Game proof deadlines and worker completion wakes

Applied game proof scheduling exposes the next refresh deadline when idle and
the current proof expiry while a replacement is in flight. Once the consumer
withdraws expired proof, it waits for the pending result. This avoids
rescheduling an already processed expiry. Process and DCS catalogue proof
limits remain separate at 25/100 ms and 125/250 ms respectively.

The launch baseline waiter bounds its wait by the proof deadline as well as
producer notifications and the existing launch deadline. The active host also
includes the next proof deadline in its scheduler. Game inventory publication
and context-resume workers wake the host after queueing results. These wakes
carry no proof or inventory authority.

The active 5 ms service fallback remains. Removing it requires a qualified
steady-state adapter and a bounded path for deferred inventory validation and
reader recovery. Physical simulator and controller latency evidence is
tracked in #199.

## Verification

- Native suite passed with 2,352 tests and 18 existing ignores. The validation,
  inventory publication and source wake modules reached 98.70%, 88.29% and
  73.91% production line coverage.
- All 14 telemetry frontend tests passed. The timing audit verified 247 sites.
- The Windows dev build, passive telemetry subscription and workspace checks
  passed. Executable SHA-256 was 80b4fe17794f8eab3dfa5f63296fee968a007a9976bf90b20426d5271f8dc99f.
- The authored Assetto run stopped during setup with Windows WMI access denied
  (0x80041003). The fixture did not reach active routing. Native process-monitor
  consent is tracked in #199.
