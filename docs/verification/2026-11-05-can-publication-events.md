# CAN publication change notifications

Temporary CAN controller publication now waits for sequenced gateway events
and the actual settlement, renewal and restoration deadlines. The actor's
repeated 20 ms State/Inspect requests are removed. A source notification
invalidates captured evidence before the next command can use it. Correlated
responses still prove private ownership of the exact lease.

The gateway CDC owner sends a baseline and committed changes. First input and
input expiry count as changes. Steady input traffic stays quiet. A DTR falling
edge retires queued bytes and stream identity, including close and reopen in
one USB task. Pending events coalesce and the writer finishes partial lines
before switching records. The firmware and POSIX simulator implement the
canonical wire contract.

Cockpit checks the boot and sequence in the parked reader and wakes the
publication actor directly. Malformed records, sequence loss and connection
replacement invalidate old proofs. A replacement connection during expected
layout settlement needs fresh private verification. An admitted run must
withdraw when its stream is lost. A proven unsent renewal can revalidate the
same lease within its original deadline. An ambiguous write stays fenced.
The running profile's health check continues to require current proof and can
withdraw a run if it observes a gap during revalidation.

## Verification

The wire contract is in protocol `e44ac5e829e6deed521066995df6050040aafa7d`.
Gateway producer and simulator changes are in device-gateway
`d5e2b3ced7571f5b91e3dc2c4feccf08bbd95a05`.

- Four gateway sanitizer and coverage suites, all six source coverage gates,
  six native CMake tests and eight POSIX simulator tests passed. Publication
  owner line coverage was 96.54%. The simulator needed Linux temporary storage
  to enforce private flash lock permissions.
- The STM32G0B1CB image built with cached Arm GNU 14.3.1 and compile-only
  `0xFFFF:0xFFFF` identifiers. It uses 70,420 bytes of flash and 119,312 bytes
  of RAM. Hardware validation remains in #199.
- All 2,337 local native tests passed, with 17 existing explicit ignores.
  Changed production gates passed at 97.73% for the publication feed, 89.76%
  for serial delivery, 93.33% for request dispatch and 89.72% for the actor.
  Eleven coverage-helper tests and all 247 historical timing dispositions passed.
- The production Windows build passed. Executable SHA-256 is
  `3355f88a9168f0587bafc2f29e2d189d23603bacf547cc42676dc6f4c3ae6ad7`.
  Running-Cockpit preview recovery, profile ownership, exact vJoy Raw Input,
  desktop scope lifecycle and browser companion routing passed. Final runtime
  cleanup left all virtual slots free and the profile stopped.

The POSIX gateway simulator and Windows cockpit were exercised separately.
General CAN gateway inventory still uses its snapshot cadence. Board-level
CDC, layout and delivery acceptance remain open in #199. The broader #188
read-model, process and telemetry maintenance, MCP and integrated verification
work remains open.
