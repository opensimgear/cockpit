# Games and hardware profiles

X-Plane 11 and 12 have separate game identities and profiles. Their read-only RREF
provider shares the existing output runtime. See [X-Plane setup, channels and
qualification limits](x-plane.md). Native launch/telemetry currently requires
Windows; live simulator and physical hardware qualification remains pending.

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
DCS World, Assetto Corsa Rally, X-Plane 11 and X-Plane 12 have separate catalogue identities.

## Integration setup and readiness

Game details and tiles report integration setup independently from installation
availability and live telemetry. **Ready for integration** can be shown while the
game is closed. Receiving/stale/waiting game data is a separate runtime indicator.

For DCS on Windows, choose its Saved Games user folder and **Set up integration**.
Cockpit installs checksum-pinned DCS-Skunkworks DCS-BIOS 0.11.7, preserves existing
Export.lua scripts and adds its loader once. It checks the actual module runtime,
JSON catalogue, loader and compatible UDP settings before reporting Ready. Close
DCS before setup or recovery; installation does not start bindings or alter hardware.

Setup keeps backups and a journal beside local integration setup settings. Cancelled
or failed operations restore matching owned files. **Recover integration setup**
preserves external edits and reports conflicts instead of overwriting them. Unknown
forks, changed module/configuration files and ambiguous loaders need attention.
**Recheck readiness** observes the current files; **Rescan** also rechecks setup.

Edit provider connection, freshness, catalogue and DCS aircraft/module settings
directly in the owning game's details. Saving preferences is separate from
installing or repairing game files. Saves require the displayed opaque setup
revision; stale forms and external settings edits cannot overwrite newer values.
Setup preserves the game-owned aircraft selection and connection preferences.
User UDP overrides that do not match the installed module remain Needs attention.
Native setup is unavailable on Linux/macOS, where profile editing remains available.

For ACR, **Set up telemetry** prepares native shared-memory connection settings;
no game plugin or telemetry enablement file is installed. Setup fingerprints the
actual runtime executable (including the Unreal child of a bootstrap executable).
The first closed-game setup is configured but unverified. Enter a live stage and
Recheck after entering a live stage. Only a valid advancing native stream verifies that exact build.
A previously verified unchanged build remains Ready while closed. Executable
updates and incompatible layouts need rechecking; access errors and conflicting
Assetto producers report their specific blocker.

Provider preferences can be saved while the game is closed with no process ID.
Native observation verifies the actual process path, PID and creation epoch;
setup preferences cannot supply that identity. PIDs are never stored in setup
records as game identity. RPM/gear/speed and optional maximum-RPM availability from
the last native check are labelled separately from live values. Missing maximum
RPM affects rev-ratio outputs only. Synthetic data exercises the adapter and never
establishes native readiness. Shared mapping names do not identify their creator;
known competing Assetto processes are rejected during checks and ongoing reads.

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

The unified profile graph owns simulator command/channel bindings and their
visible processing. Provider settings belong only in game details. Independent
DCS-BIOS/integration editing, save and start commands are retired at the native
gateway boundary; a captured integration in an old game hardware profile cannot
be launched through that alternate path. Existing hardware coordination and
explicit recovery remain available during the remaining profile cutover.

## Unified integration context

Setup readiness, verified process identity and telemetry freshness are separate
facts. Closed games retain catalogue ports and preferences. Discovering an
executable or receiving packets does not admit an integration branch.

The native coordinator admits a branch only for an immutable applied profile
owner and its matching verified game context. A manually applied profile may
have no game context while ordinary hardware routes remain useful. Process exit,
PID reuse, process replacement, profile pause, provider edits and catalogue
revision changes revoke the old context. The running integration is bound to its
original token; a newly available same-game process cannot inherit that worker.
Only the affected integration stops; ordinary mapping routes are not stopped by
this context broker.

P3 provides this native admission contract. P4 supplies profile activation and
compiled execution, so integrations remain dormant until that owner exists.
The browser's context-session label is display metadata and cannot authorize
source attachment, telemetry or commands. Editing or saving settings never starts
an integration, and settings saves revoke an affected old context before returning.

## Activation, launch and recovery

Cockpit reads the affected peripherals and records their previous settings before
applying a game hardware profile. It applies the selected hardware setup and
launches the game when requested. A provider cannot attach before actual matching
process verification. The native coordinator monitors the process independently of the
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
