# Pico Debug inventory and watchdog

Cockpit now consumes the sequenced Pico Debug inventory stream through the
existing duplex CDC reader. A baseline or inventory change triggers a correlated
snapshot. The idle two-second snapshot is replaced with a compact watchdog
renewal. Valid ordinary replies also defer the next renewal. A failed renewal,
missing baseline, sequence gap or transport loss closes the connection and
uses the existing automatic reconnect path without replaying a mutation.

Explicit reads and command-scoped layout verification still request snapshots.
Pico Debug requires the current protocol-3 firmware. Its stream conveys no
CAN publication ownership, and native HID and ordered device input delivery
retain their existing paths.

- Gateway firmware `e761e4d685324f9d7b13a3c2c2527d750fbbd854` and protocol
  `d0d6a4ce14f4f58bb7d54523aae3499d412b3414` are on main.
- Pico firmware tests pass under ASan and UBSan. GCC line coverage is 100.00%
  for `debug_gateway.c` and 99.38% for `virtual_device.c`. The Pico W image
  compiles with pinned Pico SDK 2.3.1 and Arm GNU 14.3.1.
- The POSIX firmware fixture passed idle watchdog renewal, unsolicited inventory
  notification, correlated snapshot readback and a fresh baseline after TCP
  reconnect. The harness runs the portable firmware core.
- The Windows dev build passed. The running Cockpit workspace subscription and
  delayed controller preview regressions passed. The executable SHA-256 is
  `265d021440eed118dc137786ee0441b66603d062cfec9505facd69e356dfcd6c`.
- All 153 frontend transport tests passed. The timing audit accounts for all
  247 pinned sites.
- The native suite passed 2,341 tests with 17 ignored. The Pico worker regression
  observed two idle watchdog renewals with no snapshots, then read and published
  the changed axis. All four focused native coverage gates passed. Inventory
  service measured 88.89%, workspace 95.90%, serial reader 93.08%, and the
  publication feed 97.25%.

The Windows Cockpit and POSIX firmware fixture ran separately. Physical Pico
USB behavior and input latency remain acceptance work in #199.

## Protocol-1 retirement

After the operator approved retirement, `42f733d2` removed the protocol-1
worker and its two-second status query. A protocol-1 handshake now shows
firmware upgrade instructions and parks automatic reconnection until an
explicit reconnect or a topology change.

The protocol-3 fixture exercised the production Pico selector and workspace
cache. It observed two idle watchdog renewals, a sequenced inventory change
and a clean disconnect. The Windows suite passed 2,361 tests with 18 opt-in
tests skipped. The running Cockpit passed the workspace baseline and
subscription lifecycle checks. See the [recorded decision and coverage results
in #189](https://github.com/opensimgear/cockpit/issues/189#issuecomment-6695159221).
The physical protocol-3 board check is tracked in #199.
