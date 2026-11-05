# CAN inventory change notifications

Cockpit acquires CAN inventory after the initial sequenced baseline and when
gateway evidence advances. This removes the two-second background snapshot
request. The worker coalesces source wakes, serves already queued requests
first, and bounds readback by the captured gateway authority. Explicit status
reads still inspect the gateway. Setup claims and unowned connection inactivity
keep their separate deadlines. Pico Debug retains its snapshot cadence.

The initial connection reply supplies point-in-time metadata. A gateway that
advertises publication support establishes live HID evidence only after a
sequenced readback. A lost or superseded authority fence withdraws that
evidence. Cached reads preserve the original snapshot timestamp and consume
the native fence. Its current-state flag stays private to native consumers.

An admission warning change advances inventory on the same wire sequence while
preserving publication authority. Repeated identical rejections remain quiet.
The cumulative counter is sampled in snapshot replies. A new warning arriving
during an authority readback leaves the previous warning visible until its own
readback completes. Stable HID evidence can recover independently. A new
authority change supersedes the entire old background reply.

## Verification

The admission producer is in device-gateway
`70a4d895dc732d31c977b34552bea56430a12ecf`. The wire contract is in protocol
`e0cefc23247f1bc6901611f1d9dfa3cc97154fb6`.

- Four gateway sanitizer and coverage suites, six firmware coverage gates,
  six Windows native CMake tests and eight POSIX simulator tests passed.
  The publication owner covers 230/238 lines (96.64%).
- The compile-only STM32G0B1CB image built with Arm GNU 14.3.1. It uses
  70,532 bytes of flash and 119,336 bytes of RAM.
- The native gateway worker test uses a duplex fixture through the normal
  connection and request path. It verifies startup evidence, zero quiet-window
  requests, stale reply rejection and HID recovery during a diagnostic change.
- All 27 frontend workspace tests and eleven coverage-helper tests passed.
- The default single-worker native suite passed 2,340 tests with 17 ignored.
  The earlier two-worker run failed in DCS acquisition and X-Plane subscriber
  admission. Both passed in the default run. All six affected coverage gates
  passed: inventory service 77.78%, workspace 95.90%, serial reader 92.31%,
  publication feed 97.25%, HID publication 97.70%, and profile inventory 73.62%.
- The Windows production build passed. Executable SHA-256 is
  `d87f684e33c29b2a090dacf0a416739d2bb87635677e35eb270115d1e07fcd9d`.
  Six running-Cockpit checks passed across workspace subscriptions, controller
  preview, profile ownership, exact vJoy Raw Input, desktop scope and companion
  routing. The workspace check observed no periodic frontend discovery or state
  requests during its quiet window. Final cleanup left the profile stopped and
  all virtual slots free.
- The unlocked Windows retry of the controlled keyboard focus test ended
  before sending input. Windows did not give either receiver build foreground
  focus. The separate desktop scope subscription lifecycle check passed.

Live CAN inventory requires gateway firmware that supplies the sequenced feed.
The Windows cockpit and POSIX simulator were exercised separately. Physical
CAN inventory, USB layout, and route acceptance remain in #199. Process and
telemetry maintenance, remaining producer and consumer matrices, and final
integrated qualification stay in the original #188 scope.
