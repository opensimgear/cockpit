# Unified profile resources (P1)

The native resource inventory is shared by the profile editor and its future
compiler. It composes existing host-controller, CAN, companion and integration
providers with the standalone OSG USB adapter. It does not start a profile.

## Identity and routes

A resource has a persistent local UUID. Its current HID, CAN, USB or companion
connection is separate metadata. CAN gateways are attributes of module resources,
not extra nodes or transport cables in a profile graph.

Unproven CAN sightings use gateway-scoped identities. Matching names, module IDs
or VID/PID values do not merge them. A standalone module handshake verifies its
firmware role, protocol, 32-hex module identity and 24-hex MCU USB serial. A matching
CAN announcement also requires matching boot evidence before live correlation.
HID correlation requires that verified USB identity and matching USB VID/PID and
serial. Joined records retain UUID redirects so existing local assignments remain
resolvable. Duplicate physical assignments produce diagnostics.

Two live CAN routes for a proven physical identity, inconsistent simultaneous
boots, or duplicate USB serials block routed writes. A newly acquired compatible
route prefers direct USB. Existing tokens and configuration writers stay scoped
to the captured route; discovery never moves a writer or replays a draft.

## Capabilities and unknown state

Ports preserve their provider IDs and advertise state, ordered event or atomic
action contracts. Numeric schemas retain bounds and units. Text retains byte and
encoding limits; actions retain zero through four ordered arguments, enum/RGB
constraints, display dimensions, setter/one-shot semantics and actual endpoint
ownership capabilities. Read-only integrations advertise no command ports.

OSG event transport supplies button transitions and axis snapshots. It does not
advertise hat capture. A physical HID provider can independently advertise hats.
Generated virtual controllers remain excluded by the existing input registry.

An unavailable value has no invented zero/false fallback. Provider-declared
fallbacks are distinct from profile-authored processing. Native input snapshots
capture descriptor metadata atomically, without subscribing to a new input feed.
Companion transport comes from the native adapter that owns its current session;
inventory does not assume that a companion is connected over a network.

Disconnect preserves last-known descriptors and the profile's own bindings.
Explicit removal is different: complete paired-companion, saved-controller and
gateway module inventories retire records no longer configured by that provider.
Unpairing a companion or removing its panel assignment removes its inventory
resource, including metadata retained by earlier runs. Saved profile blocks and
bindings remain unresolved until edited; cleanup does not rewrite profiles.
Offline paired companions and unplugged USB devices keep their descriptors, and
failed or partial provider reads do not establish removal.
Restart restores metadata offline under a new inventory session. A new route
needs current capability verification and a fresh baseline before it can obtain
authority. A partial or failed capability read cannot silently delete another
component's retained ports. Companion publication and CAN configuration-generation
changes also invalidate the captured contracts.

## Native cache and persistence

Discovery runs in a native background worker, independently of UI polling.
Standalone USB uses a long-lived bounded CDC worker per endpoint. Cached reads
do not acquire configuration claims or endpoint output leases. Game integrations
remain visible while closed, with setup or matching-game diagnostics; P3/P4 own
their eventual context admission and activation.

Fresh local metadata is stored separately in
`profile-resources-v1/inventory.json`, with a 32 MiB bound and atomic replacement.
It contains identities, evidence and last-known contracts, never current values,
event cursors, active leases or live routes. Corrupt metadata is retained and
reported read-only instead of being silently replaced.

Port inspection is paged. Complete native catalogues support up to 20,000 ports;
the aggregate cache supports 40,000 ports, including retained route variants.
Browser replies are bounded below the 2 MiB profile framing limit. A short page
advances by the number actually returned. Clients reject mixed inventory sessions,
revisions, resource generations or capability revisions while collecting pages.

Tokens bind inventory session, persistent resource, generation, capability
revision, exact route, provider session/generation, port contract and requested
access. Native code checks every field. A token is neither a submitted action nor
proof that a device or game accepted it.

## Standalone USB commissioning

Strict automatic discovery only probes compatible OSG metadata. On Windows,
generic USB serial driver metadata can hide the USB product strings. An explicit
port choice can use a plausible USB MCU identity, then must pass the same read-only
firmware-role and UID handshake. Unknown generic ports are never auto-probed.
Every reconnect verifies identity again and creates a new connection generation.

Configuration claims belong to one local client and captured route. Apply, Save,
Discard and release are explicit operations. Lost mutation replies are uncertain;
the adapter reconnects and reads state when possible, without resending the write.
Actions and event capabilities are advertised only from validated device replies.
An explicit unsupported reply preserves ordinary configuration access without
inventing controls or actions.

The complete `info` response requires the formatter fix in
[device-template #16](https://github.com/opensimgear/device-template/pull/16), merged
as `d9bb3095`. Earlier firmware can truncate the identity line for legal long names;
the host refuses that incomplete identity rather than guessing its fields.

## IPC and frontend

`profile_resources_list`, `profile_resources_inspect`, `profile_resources_token`
and `profile_resources_validate` return `state.profileResources` through the
existing native gateway transport. Inventory has its own frontend queue.

`module_usb_ports`, `module_usb_list`, `module_usb_connect`,
`module_usb_status`, configuration/claim operations, action inspection/invocation
and event reads return `state.moduleUsb`. The typed USB client serializes each
endpoint's requests, cancels unsent requests on Disconnect and does not retry
mutations. The native worker independently checks the captured route.

`src/lib/profile-inventory.ts` and `src/lib/module-usb.ts` define these separate
local contracts. Provider IDs such as `button:0` must be explicitly mapped to
logical graph ports with captured contract hashes. P0's scalar graph schema cannot
represent every structured action yet; P1 rejects lossy assignments. P2 extends
the graph/compiler contract, and P6/P7 deliver the list, Add flow and editor.

The profile page refreshes inventory when opened and while visible. The component
palette follows that inventory, including removal of unpaired companions.

Automated contract tests and CI builds are separate from physical acceptance.
Existing USB/identity issues #3/#4 and Windows/game/companion qualification tickets
retain their scope. Linux/macOS output and input activation gates remain intact.
