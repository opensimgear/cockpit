# Local game discovery and review

Cockpit builds its game catalogue from locally discovered installations. There
are no built-in or custom game classes. On first startup it scans known local
paths and installed launcher clients automatically. A launcher client is
included only when its own executable is present as a regular local `.exe`; the
local-path scanner remains available without a launcher. Scanning reads local
installation metadata only: it does not read account credentials, contact
client services, change launcher files or enumerate owned games that are not
installed.

The Games list reports first-scan progress and opens the review when the
automatic first scan finishes. **Rescan** opens a details view showing progress
by client and local path. Library accordions open for active scans and available
changes; their open or closed state follows the user's choice while progress
updates. The review separates new or changed installations from installations
that appear removed. Add/Update results are selected by default; Remove results
require explicit selection. **Apply** commits the selected changes atomically.
A changed path can be recognized as a moved installation, while another
installation of the same canonical title remains a separate installation.
Multiple launchers can therefore point to the same game title without creating
separate integration identities.

When metadata contains an ambiguous generic title, a user can explicitly match
that result to another pending or saved title. This supports common title
variants, but Cockpit does not promise automatic identification of every unknown
executable. Integration capabilities attach to the discovered canonical title,
independently of which installer found it. Existing integration setup and its
readiness rules are unchanged. A selected runtime executable is still required
for verified process identity, launch and game-scoped output. Executable
filenames are fuzzy-ranked against the title as suggestions; this does not
merge game identities. Users can choose another candidate, and a still-valid
explicit choice is preserved on rescan.

Discovery artwork is a bounded local preview used only in the scan review; it is
not saved by Apply. When a local image is unavailable, the review uses a local
placeholder and platform icon. The real platform SVG marks come from Simple
Icons and Font Awesome; provenance and license details are in
[`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md). Saved game tile artwork
has its own sources and override behavior, described in [Games](games.md#tile-backgrounds).

## Implemented adapters

| Client | Local evidence read | Behavior and limits |
| --- | --- | --- |
| Steam | Registry/client roots, `steamapps/libraryfolders.vdf`, installed `appmanifest_<appid>.acf` files and `appcache/librarycache` artwork | Searches primary and secondary libraries; verifies matching manifest/app identities and existing installation directories. Cached artwork includes nested app-ID directories. Manifests do not reliably classify games versus tools, so uncertain entries need review. Executable suggestions exclude installers and crash handlers; the user can choose the actual runtime. |
| Epic Games | `%ProgramData%/Epic/EpicGamesLauncher/Data/Manifests/*.item` | Uses installation location and `LaunchExecutable` as local suggestions. Excludes incomplete installations, Unreal Engine, plugins and DLC. Games requiring launcher authentication may need to be started through Epic first. Manifest launch arguments are not executed. |
| Battle.net | `%ProgramData%/Battle.net/Agent/product.db` | Reads the installed product records in the local protobuf database; excludes Agent/client entries and missing/uninstalled paths. Known products receive display names; other products use their install folder name. |
| EA app / Origin | Windows `EA Games`, `Origin Games`, `Electronic Arts/EA Games` and `Electronic Arts/EA Core/Installed Games` registry branches | Reads existing install directories in both registry views and both machine/user hives. Empty and stale records are skipped; folder names supply missing display names. |
| Ubisoft Connect | Windows `Ubisoft/Launcher/Installs/<appId>` registry records | Uses `InstallDir`, with uninstall-record display names when available and folder-name fallback. |
| GOG | Windows `GOG.com/Games/<gameId>` registry records | Uses the game's `PATH` and local display name. This also covers standalone GOG installers without Galaxy. |
| Xbox PC | `<drive>:/XboxGames/<game>/Content/MicrosoftGame.config` on fixed local drives | Reads identity, display name and PC executable from the GDK configuration. Custom install roots and protected WindowsApps packages may not be discoverable. |

The Steam adapter does not read account-wide app metadata to guess a launch
executable. A Steam installation can contain launchers, dedicated servers and
several game runtimes. If discovery cannot identify a runtime, process-based
activation and game-scoped keyboard/mouse output remain unavailable until a
runtime is selected during review or installation details.

## Local records and limits

All adapters read local records only. Missing clients are harmless; stale records,
malformed or oversized metadata and inaccessible directories are skipped. Scans
are bounded by file size, entry count, traversal depth and response size. Registry
views and repeated metadata records reuse the same client/app identity.

Steam launches through its client using its native app ID while Cockpit monitors
the selected runtime path, PID and creation epoch. Other clients use the selected
runtime directly, so games requiring authentication may need to be started in
their launcher. Discovery does not prove sign-in, entitlement, gameplay or
telemetry readiness. Manifest command-line arguments are never executed.

The EA layouts vary across games and installer generations; entries without an
installation path cannot be recovered from the registry alone. Xbox scanning
covers accessible flat-file GDK installs in conventional XboxGames libraries,
not every Microsoft Store package. A title outside supported metadata layouts or
known paths may not appear in discovery.

Implementation references: [Battle.net protobuf schema in Lutris](https://github.com/lutris/lutris/blob/master/lutris/util/battlenet/product_db.py),
[EA installer records in OpenRA](https://github.com/OpenRA/ra2/blob/master/mods/ra2/installer/origin.yaml),
[Ubisoft discovery in Vortex](https://github.com/Nexus-Mods/Vortex/wiki/MODDINGWIKI-Developers-General-Game-detection),
and Microsoft's [GDK flat-file installation](https://learn.microsoft.com/en-us/gaming/gdk/docs/features/common/packaging/packaging-flatfileinstall)
and [MicrosoftGame.config](https://learn.microsoft.com/en-us/gaming/gdk/docs/features/common/game-config/microsoftgameconfig-overview) documentation.

## Verification boundaries

Fixture coverage includes Steam secondary libraries, integrated-game identity
reuse, nested cached artwork, mismatched/duplicate manifest identities, path
traversal rejection, Epic runtime suggestions, incomplete installs and
engine/plugin/DLC filtering, Battle.net protobuf records, Windows registry layouts,
Xbox configuration parsing, stale installs and duplicate client records. Native Windows process smoke tests use private
owned helper executables to exercise exact process identity, ambiguity, exit,
replacement, observation freshness and the separate X-Plane exclusivity guard.
They do not establish successful authentication or launch behavior for every
third-party client or game.

Real Steam acceptance on 2026-10-22 also launched **CarX Drift Racing Online**
and **DiRT Rally 2.0** through Cockpit, verified their exact native process
identities and selected profile contexts, and stopped only the recorded new game
instances. This establishes launcher/process/profile composition; it does not
qualify gameplay or hardware output. Evidence is recorded in
`.research/custom-games-native/steam-final-context-guard-evidence.json`. Both
launches also rejected registration removal through the native active-context
guard, independently of the immutable hardware-first launch owner identity.

Two other installed titles demonstrate why installation metadata cannot prove
launch readiness. Steam accepted Liftoff's app ID but remained at
`ShowInterstitials`, waiting for a user response. Darkhaven Demo failed during
`RequestingLicense` with Steam's `AppError_5`. Cockpit did not report either game
as started and its native startup deadline expired. Client sign-in, launch
choices and entitlement errors remain Steam's responsibility; Cockpit never
turns these conditions into verified process evidence.
