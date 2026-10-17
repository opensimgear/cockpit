# Native profile panel adapters

`gateways::ProfilePanelIo` is private native access to the existing CAN adapters
and standalone USB Fleet workers. It shares the Manager's configuration mutex
with browser setup requests. Native configuration claims check actual private
opposite-transport writer pins, and both transports must explicitly release a
writer before a new route can claim it. UI and native claims cannot race through
separate locks. The dispatcher does not recursively call Manager IPC while the
profile coordinator owns admission.

CAN mutations require the exact current nonzero module boot. USB requests carry
the exact live module/serial/connection/boot/configuration route token and retain
the Fleet's existing per-endpoint busy and shutdown guards. Failed requests are
never replayed. Cached uncertain worker state remains visible after failures.
Native endpoint ownership operations use typed input-only USB requests and the
existing action lease protocol; ownership secrets never become public snapshots.

The native entry point cannot create or disconnect transport sessions, save
firmware settings, reset devices, forget remembered devices or replay a lost
mutation. Configuration apply is explicitly runtime-only (`save=false`). Direct
USB firmware currently advertises no HID publication enable operation; an enabled
override for that transport must remain explicitly unsupported. CAN publication
overrides use the actual gateway's exact-boot lifecycle operations and compare
captured active configuration plus publication state before restoration.

Shutdown closes public admission and joins the profile poller first. Private
native IO remains available for the coordinator's owned cleanup, then closes
its private atomic admission fence before the workers themselves shut down. A pending
new mutation that reaches this private fence after closure is rejected.


`PanelInputs` acquires read-only subscriptions through these existing workers.
`prepare(plan, io)` pins each native USB connection/boot/configuration token or
CAN epoch/node/lease/boot/configuration/input-stream tuple. `acquire(origin_us)`
starts at most sixteen isolated read loops with a shared global sixteen-request
budget, including readers retiring after Stop. The coordinator calls
`wait_baseline(delivery_plan, timeout, cached_validate, cancelled)` after genuine
inventory read tokens exist; this waits at most three seconds without holding up
Stop. `start()` then takes a new native baseline at the current journal head,
skipping all pre-activation button events. `poll(cached_validate)` only drains a
bounded mailbox and checks cached native capabilities; it never performs device
I/O. The Host's clock origin must use the same monotonic microsecond timeline as
its typed reducer.

Read frames preserve raw 0–65535 axis values, maintained button state, and ordered
button press/release occurrences. Event ports remain Unknown at baseline. Repeated
replies with a frozen device sample timestamp do not refresh analog or maintained
button freshness. Journal overflow, gaps, or inconsistent reconstructed state
invalidate continuity and require a fresh baseline; no missing event is replayed.
A stalled endpoint does not stall other endpoints. Mailbox overflow discards the
whole pending event batch and advances the local source generation before new
baselines are admitted. Stop revokes admission immediately, clears pending data,
and discards late replies without waiting for transport timeouts. The read-only
worker claims no configuration writer or action lease and has no restoration
receipts. Owned Setter dispatch and HID publication overrides remain separate,
blocked adapters until their actual advertised restoration guarantees are met.

The native Host now composes this reader into actual profile activation. Acquire
uses the controller service's monotonic timeline. Both initial and final
baselines check the real reader with bounded, cancellable waits while keeping
any acquired virtual feeder alive. Start admits subsequent panel frames, and
native health drains route-checked frames before timers or output effects.
Neutralization and preparation disposal revoke the reader immediately.

An actual Manager test acquires a retained CAN input, observes its value through
the applied graph inspector, then changes its native boot and verifies degraded
source authority without configuration or action writes. This uses deterministic
native adapters and does not establish physical hardware qualification.
