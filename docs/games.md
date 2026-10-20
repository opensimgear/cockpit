# Games and unified profiles

X-Plane 11 and 12 have separate game identities. Their read-only RREF provider
exposes typed telemetry ports to the unified profile graph. See [X-Plane setup, channels and
qualification limits](x-plane.md). Native launch/telemetry currently requires
Windows; live simulator and physical hardware qualification remains pending.

Open **Games** to discover supported installations, edit provider settings and
launch a game with the saved unified-profile selection. The initial view shows found installations. The
**Show all supported** switch reveals muted, unavailable games; their details
remain accessible for installation setup. Launch appears on hover or keyboard
focus. Native game launch and process monitoring target Windows; other platforms
can edit profiles and exercise the interface and portable fixtures.

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

Open **Profiles** for Default, saved profiles and **Add profile**. A row opens its
editor; **Back to Profiles** returns to the list. The graph owns physical input,
virtual layouts, visibility intent, companion publication selection, simulator
commands/telemetry and visible processing. Saving or navigating never activates
it. Copies exclude machine assignments and do not modify the source or Default.
See the [Profiles guide](profiles-guide.md) for the complete rig workflow.

Game details own installation/provider settings and automatic selection rules.
Manual retains an explicit saved profile pin; Automatic uses matching verified
running games, unique priority and Default fallback. There is no separate per-game
hardware-profile editor. Game/provider requirements belong to the graph and its
native admission, separate from permanent device setup and calibration.

Physical connectivity, HID publication, visibility and signal routing remain
separate properties. Unsupported temporary publication overrides are rejected;
profiles cannot simulate them by writing persistent firmware settings. Descriptor
changes may require restarting a game or revisiting its controller bindings.

## Unified integration context

Setup readiness, verified process identity and telemetry freshness are separate
facts. Closed games retain catalogue ports and preferences. Discovering an
executable or receiving packets does not admit an integration branch.

The native coordinator admits a branch only for an immutable applied profile
owner and its matching verified game context. A manually applied profile may
have no game context while ordinary hardware routes remain useful. Process exit,
PID reuse, process replacement, profile stop, provider edits and catalogue
revision changes revoke the old context. The running integration is bound to its
original token; a newly available same-game process cannot inherit that worker.
Only the affected integration stops; ordinary mapping routes are not stopped by
this context broker.

The profile coordinator supplies native admission and compiled execution;
integrations remain dormant until a matching applied owner exists.
The browser's context-session label is display metadata and cannot authorize
source attachment, telemetry or commands. Editing or saving settings never starts
an integration, and settings saves revoke an affected old context before returning.

## Activation, launch and recovery

**Launch** uses the active profile in Manual mode, or Default if none is active.
Automatic mode uses its matching rule/default. If a
profile is active, it asks for confirmation before switching. Cancelling leaves
that profile active and launches nothing. Native prepares a
saved hardware-only candidate, writes ownership/recovery intent and holds its
hardware neutral before one process spawn. Process waiting happens off-lock;
**Deactivate profile** and passive native status stay available. The independent watcher
must confirm the exact executable, PID and process creation identity before a
fresh native context can activate the graph. Browser process fields cannot grant it.
An already running external process prevents a duplicate spawn.

Optional provider setup is needed only for the selected graph's matching branches.
A hardware-only profile can launch while optional integrations remain dormant. A
higher-priority verified running game can retain the selected profile context
while another game is launched. The returned state shows that actual decision.

Deactivation cancels pending launch and releases owned output without terminating the
game. An uncertain spawn or failed post-spawn admission is not replayed; a game
may remain open while its profile requires cleanup.
Automatic selection is inhibited by deactivation, recovery and faults until the
corresponding explicit controls are resolved. Game/context loss revokes affected
integration authority; Automatic reevaluates saved rules and Default fallback.

Use **Profiles → Recover outputs** for unified session recovery. It restores
retained ownership and never restarts commands, scripts, timers or hiding from a
saved run. Offline devices and conflicting external edits can leave recovery
pending. Reconnect and inspect the reported exact blocker before trying again.
The game details recovery banner is for retained sessions from the retired
coordinator; it is separate from the active unified ProfileSession.

## Validation

`pnpm games:coverage` gates game helper lines, branches and functions above 70%.
Native discovery, profile launch and coordinator tests use authored
installation/process and peripheral fixtures to exercise activation, exit, launch
failure, offline recovery, stale identity and interrupted sessions. The native integration
coverage script also gates the new production modules above 70% line coverage.
These checks do not qualify installed Windows games, USB enumeration or assembled
hardware. Live game and physical hardware qualification remain separate work.

## Local storage and retired configuration

Manual installation overrides use a fresh bounded `game-installations-v1.json`
store. Provider setup preferences and installation recovery retain separate
`games.integration-setup.json` and `games.recovery.json` records. A corrupt
installation store remains intact and blocks game identity/launch instead of
silently falling back to a discovered executable. Hardware-only profile Apply
remains available when no retained ownership prevents it.

Old `games.json` profiles, defaults and captured integrations/mappings are not
read or migrated. An existing recovery journal is inspected only to release its
exact retained ownership. `OSG_GAMES_CONFIG` selects the anchor for the separate
setup/recovery records; it does not enable an old configuration reader.
