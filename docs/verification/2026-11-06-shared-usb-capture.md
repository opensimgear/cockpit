# Shared USB capture for preview and profile input

The native regression uses a fixture endpoint and one USB capture. It passes the
capture to the real Fleet observation bridge and to the active profile input
worker. A firmware-format frame supplies the new axis and button transitions.
The preview receives the absolute axis with the source event's clock and
capture time. The profile worker receives the same axis at the event's native
receive Instant projected onto its own clock.

After preview unsubscribe, a second axis report reaches the same active input
worker. The worker obtains the capture once and dispatches no device queries.
Existing ordered edge and receiver tests cover button occurrence delivery and
native socket framing separately.

The running Windows dev Cockpit had zero enumerated USB module endpoints.
Three actual app checks passed for the attached VKB and passive USB/CAN
admission. The positive USB fixture runs in the native test process. A
connected board run through the app and physical latency measurements remain
in #199.

## Verification

- Native suite passed with 2,351 tests and 18 existing ignores.
- Production line coverage was 86.71% for the USB profile input worker and
  79.26% for the input observation bridge.
- The running dev Cockpit checks used executable SHA-256
  `bfc0f71a23b850aff4901d0d4b26c123ea7c60fc99ccfe91d43f0d3e8c1f45ea`.
