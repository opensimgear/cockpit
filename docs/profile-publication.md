# Profile controller publication

Controller publication decides whether a compatible CAN module contributes a
controller collection to its gateway's USB descriptor. It is separate from
physical connectivity, graph routes and HidHide visibility.

A profile may keep the configured choice or request Enable/Disable while active.
Only STM32 CAN gateways advertising the exact nonpersistent publication lease
are admitted. Direct USB, Pico, older firmware and resources with competing USB
or HID publisher routes remain unsupported. Descriptor changes can reconnect the
gateway's controller collections and require game rebinding. Device flash
settings remain unchanged.

Native preparation captures exact physical assignments, gateway/module identity,
boot, configuration and descriptor targets without mutation. Each gateway's
private owner and original target tuple are sealed in the existing Coordinator
journal before Claim. Workers are bounded to eight gateways, with at most 32
targets per gateway and 64 choices per profile. Claim and Renew require the
original control epoch and a live native run latch immediately before dispatch.
An ambiguous mutation is never automatically replayed.

Apply waits for private ownership inspection, settled layout and actual fresh
native input baselines. Only a proof from that actor can refresh its exact CAN
delivery lease and recompile acquisition pins; unrelated prepared resources, ports,
catalogues and physical identities cannot change. The authored program stays
the same. Refreshed action cleanup receipts are persisted before acquisition.
Two profiles can use different graph names for the same exact physical device.
If saving refreshed receipts fails before Acquire, cleanup validates the durable
seals and discards only action preparations whose drivers were never dispatched.
Once Acquire was attempted, cleanup still requires exact captured receipts.

Stop, Default and switching first restore owned physical actions, then
release publication. Firmware restores only the still-owned original layout;
foreign ownership or external edits are preserved and reported as recovery
conflicts. Cleanup waits are bounded and retain sealed receipts when a device is
offline. Explicit recovery performs read-only identity/private-owner checks and
cleanup only; it cannot claim a new publication, resume a graph or replay input.
Confirmed restoration requires settled readback of the captured original tuple.
Neutralization immediately withdraws Claim/Renew authority, including when
action cleanup fails. Publication Release still waits for action restoration;
an unresolved run cannot keep extending its firmware lease. Repeated Stop
retains a completed restoration proof for a subsequent profile handoff.
If a physical action cleanup worker has already failed, only explicit Recover
can reconstruct its cleanup actor after verifying the durable journal and exact
sealed receipts. The replacement starts revoked, inspects the existing private
lease and can release it; ordinary Stop cannot retry that uncertain release.

Software tests and cross-firmware simulation establish these protocol and
coordinator boundaries. Physical USB/CAN descriptor transitions, driver behavior
and game binding continuity still require actual device/game qualification.
