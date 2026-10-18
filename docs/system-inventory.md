# Devices: system inventory and connections

Devices is the system inspection entry point. List and Topology use the same
component identities, status, filters, selection and details. The separate
Gateways and Topology navigation sections have been removed. The remembered
presentation does not change connections or runtime configuration.

List presents a responsive grid of component tiles. Confirmed offline tiles use
muted surfaces, names and icons while retaining readable status and actions.
Hidden-but-present, unavailable and unknown states remain distinct from offline.
Inventory updates run automatically in the background; there is no manual refresh
button. Selecting a tile or topology node opens its details in place of the
inventory. Back to Devices restores the previous view, filters and selection.

## Inspecting the system

Every supported provider contributes its known components: third-party
controllers; OSG modules over direct USB or CAN; gateways; virtual vJoy targets;
integrations; paired companion installations, their assigned controller
instances, USB endpoints and pending pairing requests. Hidden, remembered,
unavailable and unknown components remain visible. Type counts distinguish
software components from physical devices; HID collections remain capabilities
of the physical component when exact native association evidence is available.

Search and filter by component type/state or connection transport. Topology
retains upstream host/gateway/installation context for matching components.
Select any node for the same details/actions as the list. Drag or use arrow keys
to pan, scroll or use +/- to zoom, and use Fit/Home for the full system overview.

Connected, hidden, disabled and available for input are independent facts.
Details show the observation source/time, first seen and last seen connected;
an unavailable inspection is unknown, not proof of disconnect. OSG details
separate input ownership from the current configuration writer and its route.
Current USB/CAN/network/Bluetooth links, last-known attachments, software
relationships and unconfirmed configured endpoints have explicit labels.
CAN membership does not imply a physical daisy-chain order. Unknown connections
leave the component visible without inventing a wire.

Gateway details retain retry, deliberate disconnect/connect, module setup,
controller enable/disable, older-firmware registration/layout Apply and
diagnostics. Direct USB details offer explicit connection, setup claim,
configuration Apply/Save and advertised output actions. Controller details
retain local naming/roles, input monitoring and visibility inspection. Integration
and companion actions lead to their existing editors. External controllers do
not receive OSG firmware controls.

## Forget and rediscovery

Forget is available from shared details or eligible bulk cleanup. Confirmation
explains the provider-specific effect and affected retained binding references.
Eligibility is refreshed immediately before each one-shot provider operation.
Hidden/present, online or owned components cannot be treated as stale hardware.

- Local cleanup forgets remembered metadata; it does not delete user bindings.
- Reachable gateways can forget eligible offline module records in persistent
  storage. Published USB layout changes may require game rebinding.
- Forgetting an unavailable gateway removes local stale observations and sole-route
  children, retaining independent USB/other-gateway connections. Remote flash
  cleanup cannot be confirmed while that gateway is unavailable.
- Explicit companion revocation removes pairing/assignments and requires pairing
  again. Assigned instances are not silently revoked by generic metadata cleanup.
- Virtual metadata cleanup does not delete OS devices, driver layouts or HidHide
  rules, and does not uninstall drivers.

Successful cleanup and suppression of unchanged stale snapshots persist across
restart. Genuinely rediscovered automatically detected devices can reappear.
Partial bulk failures are reported independently; ambiguous mutations are never
replayed. Failed storage writes do not publish successful durable cleanup.

## Shared contract and provider authorities

`system-inventory.ts` defines logical components, independent state, connection
observations, supported targets, references, provenance and timestamps. The
registry persists only presentation history/tombstones under
`osg-system-inventory-v1`; existing native stores remain authoritative for device
configuration, pairing, bindings and ownership. Both renderers consume the same
projection from `system-inventory-providers.ts`.

Existing paged controller/USB inventories and the common native resource facade
supply discovery and verified USB/CAN correlation. Names, VID/PID and matching
module IDs alone cannot merge sightings. Exact native gateway serial plus USB
interface/report evidence associates published HID collections with their source.
Per-gateway addresses, simulated Pico namespaces, drafts and route ownership are
preserved. Known identity aliases are migrated so provider outage/recovery does
not retain duplicate physical nodes.

`system_inventory` supplements these services with read-only PnP/HidHide and
virtual-device evidence. Its bounded pages use `offset`, `limit <= 32` and an
`observedAt` token; hidden records and controller associations share one cursor.
Replies remain below the ordinary bridge budget. Complete pagination and
snapshot consistency are checked before publication. Partial providers remain
independent and freshness/errors are visible. Proven parent bus evidence is used
for physical USB/Bluetooth links; a USB radio does not turn a Bluetooth controller
into a direct USB device.

Virtual feeder ownership and creation provenance are separate. Cockpit creation
is asserted only from a committed protected provisioning journal matching the
current native descriptor. Configured targets remain known when SDK inspection
fails. Inventory rendering never acquires/provisions an output, activates a
profile, writes driver state or changes visibility. Existing qualification and
single-writer gates remain in force. Profile requirements are deferred from this
feature; existing saved assignments are read only for Forget impact disclosure.

## Automated acceptance

Run `pnpm system-inventory:test`, `pnpm system-inventory:coverage`, `pnpm check`
and the existing transport/profile regression checks. `pnpm devices:ui` drives
the running dev Cockpit with deterministic IPC, while `pnpm devices:native:ui`
uses its actual native bridge. Configure the endpoint/runtime paths as described
in the [verification record](verification/2026-10-18-unified-devices.md).
Hardware/driver/game qualification is recorded separately from fixtures.
