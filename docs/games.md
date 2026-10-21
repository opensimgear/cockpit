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
DCS World, Assetto Corsa, Assetto Corsa Competizione, Assetto Corsa EVO,
Assetto Corsa Rally, X-Plane 11 and X-Plane 12 have separate catalogue identities.
Each tile uses bundled publisher imagery; image sources are recorded in
[`public/game-art/sources.json`](../public/game-art/sources.json).

## Vehicle discovery

Game details show the current aircraft or car from fresh native telemetry. Vehicle
observation runs while the configured game is running, including before a profile
is applied. DCS reads the DCS-BIOS aircraft name, the Assetto games read the shared-memory car
model, and X-Plane reads the aircraft's relative `.acf` path through RREF. Process
exit, stale telemetry, connection failure and a changed game session clear the
current vehicle. Selecting an aircraft tab in the profile editor browses its
catalogue without claiming that aircraft is loaded or changing the active module.

The DCS vehicle list comes from the installed DCS-BIOS aliases. X-Plane scans the
selected installation's Aircraft folder, including add-on aircraft. For the Assetto
games, Cockpit remembers exact car models observed in live sessions. Load a car once to make it selectable later,
or enter its exact native ID in the profile tab. Discovered vehicle names persist
locally; a saved discovery never restores a vehicle as currently loaded.

Vehicle lists refresh in the background at most every 30 seconds, or when the
installation or provider settings change. DCS telemetry subscribers share one
UDP receiver with separate bounded queues, so vehicle observation also works
with unicast exports while a profile is running.

DCS resolves the fresh native aircraft identity through the installed
`AircraftAliases.json` and selects its DCS-BIOS module automatically. Game details
show the active module or **Waiting for aircraft**. Each profile aircraft tab has
its own controls and telemetry catalogue; Base uses **CommonData** shared flight
telemetry. Aircraft changes revoke the previous aircraft's contributions before
the next aircraft supplies fresh data. Unknown or ambiguous aircraft aliases do
not select a guessed module. Saved module preferences do not override the loaded
aircraft.

## Integration setup and readiness

Game details and tiles report integration setup independently from installation
availability and live telemetry. **Integration ready** can be shown while the
game is closed. Receiving/stale game data is a separate runtime indicator; waiting
for game data is omitted from the UI.

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

Edit provider connection, freshness and the installed catalogue directory
directly in the owning game's details. Saving preferences is separate from
installing or repairing game files. Saves require the displayed opaque setup
revision; stale forms and external settings edits cannot overwrite newer values.
Setup preserves connection preferences. Aircraft selection follows live native
telemetry, and browsing an aircraft tab does not save provider preferences.
User UDP overrides that do not match the installed runtime remain Needs attention.
Native setup is unavailable on Linux/macOS, where profile editing remains available.

For Assetto Corsa, Competizione, EVO and Rally, **Set up telemetry** prepares native shared-memory connection settings;
no game plugin or telemetry enablement file is installed. Setup fingerprints the
actual runtime executable (including the Unreal child of a bootstrap executable).
The first closed-game setup is configured but unverified. Enter a live driving
session and refresh integration state. Only a valid advancing native stream verifies that exact build.
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

Original Assetto Corsa can retain a menu bootstrap after starting a child of the
same `AssettoCorsa.exe`. Native observation selects the leaf only when all matching
launchers share one executable path and form a single verified parent chain.
Independent launcher instances remain ambiguous. When `acs.exe` or `acs_x86.exe`
is running, its driving process takes precedence. The menu never provides native
telemetry authority.

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

Use **Profiles → Recover** for unified session recovery. It restores
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

`pnpm profiles:dcs:aircraft:native` runs the opt-in aircraft catalogue acceptance
against the running dev Cockpit. Set `OSG_DCS_VEHICLES_NATIVE=1`, start the native
dev app with its CDP endpoint available, and set up its actual DCS-BIOS integration
first. The test uses installed catalogues, a verified fixture process and authored
UDP exports to check tab catalogues, automatic module changes and stale-data
fencing. Its evidence does not qualify a flight in the simulator.

### Actual Assetto driving capture

Start the native dev Cockpit with its CDP endpoint available at port 9222, then
enter a live driving session in the actual installed game. The capture runner
uses the running Cockpit's native gateway and never launches a publisher, selects
an alternate installation, or sends game inputs:

```powershell
node scripts/assetto-live.mjs ac --seconds 15 --min-speed 5 --require-change --require-all --keep-profile --out .research/assetto-live/ac-driving
```

Use `acc` or `evo` for Competizione or EVO. Capture first selects Manual mode and
stops the previous applied profile, then runs native setup/check and creates an
applied profile containing the five read-only telemetry channels. It records the
actual executable SHA-256 and version, Windows process identity, car identity,
provider revisions, repeated delivered values, and a Cockpit screenshot in the
chosen directory. `--require-all` requires RPM, gear, speed, maximum RPM and rev
ratio together in at least 80% of samples; it also checks all five withdraw in
pause/stale/exit observations. Without it the last two remain optional. `--min-speed 5` requires
motion, while `--require-change` requires at least 100 RPM or 1 km/h of variation
and excludes small idle physics jitter. Add `--wait-speed --min-speed 5` to start
recording only after the requested channels are known and the car reaches
5 km/h. `--timeout-ms 180000` allows time to drive before that trigger. Once
recording begins, every subsequent sample is retained, including later pauses
or stops. The capture profile is removed afterward
unless `--keep-profile` is supplied.

With that profile retained, the following commands observe transitions established
in the actual game without pausing, suspending, exiting, or sending inputs to it:

```powershell
node scripts/assetto-live.mjs ac --mode observe --from .research/assetto-live/ac-driving/evidence.json --require-all
node scripts/assetto-live.mjs ac --mode pause --from .research/assetto-live/ac-driving/evidence.json
node scripts/assetto-live.mjs ac --mode exit --from .research/assetto-live/ac-driving/evidence.json
```

`--mode stale` checks withdrawal after telemetry stops advancing. Exit checks the
driving process and its telemetry; Assetto Corsa's menu may remain open. Add
`--require-game-closed` to require the menu to close as well. After restarting the
driving runtime, use `--mode reapply --from <capture>/evidence.json --require-all`
to stop the previous context, recheck the current build, and reapply the same
saved profile with fresh process evidence. A previous process epoch cannot grant
a restarted simulator integration authority. Use `--mode cleanup --from <capture>/evidence.json` to stop and remove the runner's saved profile, or
`--mode snapshot` for a read-only installation/process snapshot.

`node --test scripts/assetto-live.test.mjs` covers observation assertions and the
stop-before-setup ordering without contacting the running Cockpit.

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
