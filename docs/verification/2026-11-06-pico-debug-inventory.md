# Pico Debug inventory and watchdog

Cockpit now consumes the sequenced Pico Debug inventory stream through the
existing duplex CDC reader. A baseline or inventory change triggers a correlated
snapshot. The idle two-second snapshot is replaced with a compact watchdog
renewal. Valid ordinary replies also defer the next renewal. A failed renewal,
missing baseline, sequence gap or transport loss closes the connection and
uses the existing automatic reconnect path without replaying a mutation.

Explicit reads and command-scoped layout verification still request snapshots.
The legacy protocol-1 Pico adapter retains its original cadence. Pico Debug
requires the current firmware. Its stream conveys no CAN publication ownership,
and native HID and ordered device input delivery retain their existing paths.

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
