# Games and hardware profiles

Open **Games** to discover DCS World and Assetto Corsa Rally, select a hardware
profile and launch a game. The initial view shows found installations. The
**Show all supported** switch reveals muted, unavailable games; their details
remain accessible for installation setup. Launch appears on hover or keyboard
focus. Native game launch and process monitoring target Windows; other platforms
can edit profiles and exercise the interface and synthetic integration sources.

## Discovery

Cockpit checks Steam libraries and the supported games' installation records,
then verifies the game's executable. DCS standalone installations are also
supported. **Rescan** refreshes discovery. In a game's details, set an absolute
executable path for a custom installation, or clear the override to return to
automatic discovery. Finding an executable does not establish telemetry readiness.
Only DCS World and Assetto Corsa Rally are in the supported registry.

## Profiles

Each game has a default hardware profile. Create additional named profiles and
choose which one Launch uses. A profile can leave a controller unchanged, enable
it or disable it, and capture a connected peripheral's current configuration for
editing. Device assignments use both gateway and module identity. Identical
module identities on different gateways remain separate peripherals.

Captured settings are copies: editing or saving a game profile does not apply
settings to a peripheral. Activation uses runtime Apply without saving device
flash. USB controller enable/disable changes use the existing firmware lifecycle,
which persists controller publication. Descriptor changes can affect bindings or
require a game restart. Review and acknowledge that consequence before activation.
Re-enabling a controller appends it to the firmware's controller order; restoring
enabled states does not guarantee unchanged descriptor positions.

Configure and save a binding profile in **Integrations**, then attach a copy to
the game profile. DCS uses DCS-BIOS; Rally uses the Assetto Corsa Rally provider.
The integration owns simulator input/output bindings and sends fresh game-state
updates to hardware through the existing endpoint leases, per-gateway workers
and fallback rules. Game profiles coordinate configuration, controller state
and integration activation. They are separate from firmware HID profiles and
portable integration documents. One integration runs at a time.

## Activation, launch and recovery

Cockpit reads the affected peripherals and records their previous settings before
applying a game profile. It stops the previous integration, applies the selected
hardware setup and starts the profile's integration. Launch then starts the game;
live Rally shared memory attaches to the discovered game's actual process ID
after startup. The native coordinator monitors the process independently of the
Games view. Game exit, failed launch and **Deactivate** restore the previous setup.
Deactivation does not close the game.

A recovery journal is written before hardware changes. If Cockpit closes or
crashes mid-session, **Recover** reconciles that journal against current hardware
readback. Offline devices or conflicting external edits leave recovery pending
with a visible error. Reconnect affected peripherals and review the error before
retrying recovery. Ambiguous writes are not blindly replayed. Device-side output
leases still apply their fallbacks if the host disappears.

If an external edit or a new device boot makes restoration inappropriate, use
**Keep current setup** and confirm that choice. Cockpit archives the recovery
record and releases the session's settings guard without changing peripherals or
integration settings. The archived record remains beside the settings file.

Settings and the journal live beside Cockpit's integration configuration. For
isolated development, `OSG_GAMES_CONFIG` selects the game settings file and
`OSG_DCS_BIOS_CONFIG` selects the existing integration configuration directory.
Imported or saved settings do not automatically activate a game or integration.

## Validation

`pnpm games:coverage` gates game helper lines, branches and functions above 70%.
Native discovery and coordinator tests use authored installation/process and
peripheral fixtures to exercise successful activation, exit, launch failure,
offline recovery, stale identity and interrupted sessions. The native integration
coverage script also gates the new production modules above 70% line coverage.
These checks do not qualify installed Windows games, USB enumeration or assembled
hardware. Live game and physical hardware qualification remain separate work.
