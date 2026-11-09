# ESP32 direct USB status-watch requirement

The operator chose to retire status-only ESP32 direct USB support for now
and defer further firmware changes until ESP32 work resumes.
This is a Cockpit host change. A correlated discovery or recovery snapshot
must advertise `statusEvents: true` before the host can connect the board or
publish ready status. Automatic reconnect applies the same capability check
to the saved, paired device identity. The explicit firmware installer can
still identify a previously paired board for an upgrade.

Current boards use the existing identity-bound status watch. A small ping
renews a live watch, while a correlated full snapshot remains available for
discovery and identity-bound recovery. The former one-second status-only
operation for older firmware is retired.

The serial fixture sends unsolicited link messages and a stale discovery
response before a correlated legacy response. The worker rejects the old
device with an upgrade message before opening a Hub link, sending Connect or
publishing readiness. The current firmware fixtures exercise the watch
handshake, liveness, response fencing, wire requests and recovery. Physical
USB qualification on both boards remains in #199.

The local serial suites passed 63 tests on Windows and 60 on Linux. The
Windows native companion and firmware resolver suite passed 64 tests. Both
running Cockpit clients passed the companion roster hide and reopen check.
The fixture verified zero hidden status reads and rejection of a retired
reply. Reconnect measured 75% production line coverage and 73.33% LLVM
function coverage. Instrumentation confirmed execution of the early link
guard and the legacy rejection path.
