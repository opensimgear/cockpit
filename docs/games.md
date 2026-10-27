# Games and unified profiles

X-Plane 11 and 12 have separate game identities. Their read-only RREF provider
exposes typed telemetry ports to the unified profile graph. See [X-Plane setup, channels and
qualification limits](x-plane.md). Native launch/telemetry currently requires
Windows; live simulator and physical hardware qualification remains pending.

Open **Games** to review installed games discovered from launchers and local
installation paths, edit provider settings and launch a game with the saved
unified-profile selection. Every installed game uses the same metadata, launch
and profile flow; optional telemetry integrations add capabilities where available.`nOn first startup Cockpit scans
all installed supported clients and known paths automatically. A launcher client
appears only when its own executable is present as a regular local `.exe`; local
path discovery remains available without a launcher. The game list shows scan
progress, then opens the first-scan review automatically when it is ready.
**Rescan** opens a details view with progress by client and local path. Library
accordions open for active scans and available changes, and retain the user's
open or closed choice as progress updates. Native game launch and process
monitoring target Windows; other platforms can edit profiles and exercise
portable fixtures.

Each discovered game's title is its canonical identity, including when multiple
clients or installations expose the same title. The review step lets you resolve
ambiguous generic names by explicitly matching them to another pending or saved
title. Cockpit does not claim that every unfamiliar executable can be identified
automatically. An installation without a verified runtime cannot provide process
identity for launch, automatic selection or scoped output.

## Tags

Games can have multiple tags. The initial, editable taxonomy contains 47 major
genres and styles, including Racing, Flight, Simulation, FPS, RPG, RTS, Strategy,
Survival, Sports and VR. Discovery uses Steam Store genres as editable defaults
when a game's tags have never been assigned. It maps exact genre labels to the
current taxonomy, including renamed starter tags, and shows proposed tags in
the discovery review before Apply. Existing untagged imports can receive these
defaults on rescan. Editing or clearing a game's tags prevents future discovery
from replacing that choice, even for another installation of the same game.
Deleting taxonomy entries never recreates them from platform metadata.

Steam metadata is cached locally and fetched with bounded requests while the
library shows preparation progress. Offline or unavailable metadata does not
prevent importing games; a later rescan can fill missing defaults. When platform
genres do not map to the current taxonomy, Wikidata supplies fallback genre
facts for Steam, Epic, Battle.net, EA, Ubisoft, GOG, Xbox and local discoveries.
Only uniquely identified video games qualify: exact supported store identifiers
take priority, followed by exact English or language-neutral labels and aliases. Ambiguous titles
and unavailable records stay untagged. Integrations and executable names do not
imply tags. Suggested Wikidata tags identify their source in the review.

The **Game tags** field contains selected badges. Type to search, select a
suggestion or press Enter to add it, and use a badge's remove button or Backspace
in an empty field to remove a tag. **Manage** opens a multiline badge field:
type a name and press Enter to add it, or select a badge to rename it. Deleting
a badge shows how many games use that tag and asks for confirmation. Changes
apply when you save. Renaming preserves assignments; deletion preserves each
game's remaining tags. Cancelling leaves the taxonomy unchanged.

The game list can filter by any selected tag, including **Untagged**, alongside
library and integration filters. Tag names and assignments are saved locally.
MCP exposes `game_tags_list`, `game_tag_definitions_save` and `game_tags_save`;
both saves require the expected current values and an operation ID. Run
`OSG_GAME_TAGS_NATIVE=1 pnpm games:tags:native` against the native dev cockpit for
the tag UI, persistence and MCP acceptance check.

## Shared game metadata

The reusable `games/metadata` provider retains Wikidata entity IDs, titles,
descriptions, aliases, genres and their parent genres, platforms, developers,
publishers, release dates, external identifiers and source provenance. These
facts remain separate from editable library names, paths, images and tags.
Wikidata structured facts are CC0; linked artwork is not imported or licensed
by this feature.

MCP and the native gateway expose the passive `game_metadata_get {gameId}` read.
It returns cached rich records and match/staleness information, performs no
network request, and never changes tag assignments. Discovery populates the
cache even when a game already has manually edited or explicitly cleared tags.

Positive records expire after 30 days; negative/ambiguous matches after one day.
Offline refreshes may use an explicitly marked stale positive record. Cache
files, responses, batches and discovery work are bounded. Wikidata requests
include Cockpit's name and contact URL, run one at a time at most once per second,
and reserve at most 28 seconds of request time per rolling minute. HTTP 429/503
`Retry-After` delays (including dates) persist across restarts; other failures
use exponential backoff. Long cooldowns never hold discovery open. Future
consumers reuse this provider instead of querying Wikidata from tag controls.

## Tile backgrounds

Every game's **Installation** card includes a tile preview with **Update image**
underneath. It opens the image form with the current image ready for positioning.
Choose **From disk** to browse
to a PNG, JPEG or WebP image (or paste its path), or **From URL** to enter an HTTP
or HTTPS image URL. **Use image** loads the image into the editor inside the same
form without replacing the saved background.

The popup shows the complete image and a frame matching the tile's 16:9 shape.
Drag the frame to move it or drag any of its four corners to resize. Arrow keys
also move/resize the focused control; Shift uses larger steps. The image keeps
its aspect ratio and the entire frame stays within its bounds. **Fit image**
restores the largest crop that fits. **Save background** commits the image and
crop together; **Cancel** leaves the current background unchanged.
Images follow this priority: manually assigned image, discovered client artwork,
then a neutral placeholder. **Reset to default**, beside **From URL**, returns to
the highest-priority available image.

Cockpit copies a bounded thumbnail into its application storage. Moving the
original source file or changing the source URL cannot break the saved background. Rescans,
renames and executable relocation preserve overrides. Invalid images leave the
previous background intact. Sources are limited to 16 MiB and 8192 pixels per
dimension; managed thumbnails are at most 12 KiB. Profile export contains neither
local image paths nor image bytes.

See the [client discovery and feasibility matrix](launcher-discovery.md) for
local metadata sources and coverage limits. Run
`python scripts/game-library-coverage.py` for native adapter coverage. With the
actual native dev Cockpit running and WebView debugging on port 9222, run
`OSG_GAME_DISCOVERY_NATIVE=1 pnpm games:discovery:native` for discovery UI
acceptance, or `OSG_GAME_DISCOVERY_NATIVE=1 pnpm games:discovery:mcp:native`
for the real MCP SDK HTTP acceptance. These are opt-in native checks; this guide
makes no claim that they have passed.

## Discovery and review

Discovery scans local metadata from installed Steam, Epic Games, Battle.net, EA
app/Origin, Ubisoft Connect, GOG and Xbox PC clients, plus known local
installation paths such as DCS standalone. Launcher clients are exposed only
when the launcher executable exists as a regular local `.exe`. Findings appear
in two lists: new or changed installations, and installations Cockpit believes
were removed. Add/Update entries are preselected; Remove entries require manual
selection. **Apply** commits the selected changes atomically. The review
distinguishes a moved installation from adding another installation of the same
canonical title. Integration capabilities bind to the discovered title
independently of installer, and existing integration setup is unchanged. A
selected runtime is still required before process-based proof is available.
Fuzzy filename ranking suggests likely runtimes for review without merging game
identities; the user can choose a different candidate, and a still-valid
explicit choice is preserved on rescan. Finding an executable does not establish
telemetry readiness. Scan
artwork is a bounded, transient local preview and is not saved by Apply; when
there is no image, the review shows a local placeholder and platform icon.
Platform icon artwork provenance and licenses are listed in
[`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md).
Optional telemetry integrations are available for some installed games; integration
availability does not create a separate game class.

## Vehicle discovery

Game details show the current aircraft or car from fresh native telemetry. Vehicle
observation runs while the configured game is running, including before a profile
is applied. DCS reads the DCS-BIOS aircraft name, the Assetto games read the shared-memory car
model, and X-Plane reads the aircraft's relative `.acf` path through RREF. Process
exit, stale telemetry, connection failure and a changed game session clear the
current vehicle. Selecting an aircraft tab in the profile editor browses its
catalogue without claiming that aircraft is loaded or changing the active module.

The DCS vehicle list comes from the installed DCS-BIOS aliases. X-Plane scans the
selected installation's Aircraft folder, including add-on aircraft. Original
Assetto Corsa scans its installed `content/cars` during **Set up telemetry** and
**Repair setup**, including complete mod cars and variants. Folder names remain
the exact native car IDs; `ui/ui_car.json` supplies their display names. The saved
list is specific to the selected installation and is available with the game
closed and after Cockpit restarts. Run setup/repair again after adding or removing
cars; ordinary status checks and telemetry checks do not rescan car content.
This list describes installed content, rather than DLC ownership.

For Competizione, EVO and Rally, Cockpit remembers exact car models observed in
live sessions. Load a car once to make it selectable later, or enter its exact
native ID in the profile tab. Discovered vehicle names persist locally; a saved
discovery never restores a vehicle as currently loaded.

The DCS and X-Plane vehicle lists refresh in the background at most every 30
seconds, or when the installation or provider settings change. DCS telemetry subscribers share one
UDP receiver with separate bounded queues, so vehicle observation also works
with unicast exports while a profile is running.

DCS resolves the fresh native aircraft identity through the installed
`AircraftAliases.json` and selects its DCS-BIOS module automatically. Game details
show the active module or **Waiting for aircraft**. Each profile aircraft tab has
its own controls and telemetry catalogue; Base uses **CommonData** and the managed
read-only ownship telemetry catalogue. Aircraft changes revoke the previous aircraft's contributions before
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
Export.lua scripts and adds its loader once. It also installs the managed
`Scripts/OpenSimGear/DCSPhysics.lua` ownship extension; existing DCS-BIOS-only
setups need **Repair setup** to add it. The extension chains existing export
callbacks, reads selected documented ownship getters and sends bounded local UDP
telemetry at up to 20 Hz on `127.0.0.1:7781`. It does not send simulator commands,
enumerate other aircraft or expose weapon targeting. Cockpit checks the actual module runtime,
JSON catalogue, both loaders, managed ownship source and compatible UDP settings before reporting Ready. Close
DCS before setup or recovery; installation does not start bindings or alter hardware.

The ownship catalogue adds 101 native fields for flight parameters, motion,
attitude, engines/fuel, wind/pressure, ownship position/identity, simulation state
and mechanics. DCS engine `RPM.left/right` is percentage, exposed separately from
absolute RPM. Unsupported SDK fields remain unknown; undocumented native units
and axes are preserved without guessed conversions. Live values require fresh
matching aircraft identity, ownship export permission and advancing simulation
time. See [DCS ownship coverage and units](telemetry.md#dcs-ownship-export) for the
field groups, source reference, freshness and protocol limits.

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
