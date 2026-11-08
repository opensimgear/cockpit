# Shared Windows process evidence for active telemetry

An active Windows telemetry reader uses the same scoped process inventory
as automatic discovery. With Windows process events off or denied, one
background owner supplies the existing one-second discovery checks. The
active telemetry validator no longer starts its own process enumeration.
The inventory proves uniqueness and the exact retained process handle
proves liveness. If the exit watch is unavailable, the validator can check
the exact process without enumerating candidate processes again.

Each result retains its scan start, library and process epochs, event
coverage and exact integration selection. Fallback rows retain the
observer's existing two-second maximum age. A new short proof must fit
inside that original window. Reusing the row never advances its scan start.
Retired event coverage cannot convert an event row into a fallback row.
The validator checks the current scan again after verifying liveness,
so an already superseded result cannot authorize a new proof.

A current absent, replaced or ambiguous process withdraws the game
branches. Missing or unavailable evidence does not start a new scan or
renew the old proof. An exact process exit still wakes the reader.
The background owner wakes routing after publishing the result.

The short active proof still checks preparation and owner state. Explicit
Apply, baseline capture and simulator command authorization keep strict
checks. The DCS catalogue validator and the automatic reacquisition path
have separate qualification.

## Local verification

The focused Windows native suite passed 71 tests with one child-only
fixture ignored. The new case reuses a real retained Windows process
handle, verifies that repeated reads retain the original scan start and
rejects stale, missing, replaced and ambiguous observations. It verifies
that shared telemetry performs no platform lookup while the independent
strict command check still does.

The broad Windows suite passed 2,476 tests with 18 default ignores.
The STM32 deadline regression and two checksum-pinned DCS archive tests
passed separately. The four changed production modules reached
85.64–98.97% production line coverage and 76.32–95.65% LLVM function
coverage. The Windows instrumented build emitted no branch counters.

The Windows development candidate ran with default process-event settings
and a disposable Cockpit database. The same owner completed a 120.011
second authored soak with 61 MCP updates and four discovery scans. Both
loopback panels met their p95 budget at 48.36 ms and 48.37 ms. Windows
Raw Input observed both vJoy releases 40.50 ms after MCP Stop submission.
The slot returned to free, the fast MCP reader received retirement and
the unread response closed. Telemetry withdrew on exact process exit.
The fixture reported no cleanup errors or budget misses.

The executable SHA-256 was
`2c79ca7db75954afffc6d4b745f5c940aad72d0edb0c937e98d0f4a51de81e3d`.
The candidate linked from the isolated source under a local executable
name while the comparison build still ran. Evidence is in
`.research/dcs-physics-native/1794173528032/evidence.json`.

The [30-minute comparison](2026-11-08-active-soak-measurements.md#thirty-minute-windows-run)
ran the previous executable. It also passed, so the duplicate process
enumeration is removed independently of the earlier intermittent
withdrawal. That failure's cause remains open.
