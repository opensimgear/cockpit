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
