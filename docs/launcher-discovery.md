# Local game-client discovery

Cockpit imports registrations from local installation metadata. Scanning does not
read account credentials, contact client services, change launcher files, or add
games automatically. This is an installed-games list; it does not enumerate owned
games that have not been installed.

Each imported registration receives a stable custom identity, or reuses an
existing integrated game by its Steam app ID. Reimporting the same client/app ID
reuses its registration. Selecting an executable already registered as a custom
game reuses that game's identity and name. The monitored executable is separate
from Steam's launch target: Steam starts with its native app ID while Cockpit
observes the exact selected runtime path, PID and creation epoch.

## Implemented adapters

| Client | Local evidence read | Behavior and limits |
| --- | --- | --- |
| Steam | Registry/client roots, `steamapps/libraryfolders.vdf`, installed `appmanifest_<appid>.acf` files and `appcache/librarycache` artwork | Searches primary and secondary libraries; verifies matching manifest/app identities and existing installation directories. Cached artwork includes nested app-ID directories. Manifests do not reliably classify games versus tools, so unknown entries need review. Executable suggestions exclude installers and crash handlers; the user can choose the actual runtime or add an unavailable placeholder. |
| Epic Games | `%ProgramData%/Epic/EpicGamesLauncher/Data/Manifests/*.item` | Uses installation location and `LaunchExecutable` as local suggestions. Excludes incomplete installations, Unreal Engine, plugins and DLC. Imports launch the selected runtime directly; games requiring launcher authentication may need to be started through Epic first. Manifest launch arguments are not executed. |

The Steam adapter does not read account-wide app metadata to guess a launch
executable. A Steam installation can contain launchers, dedicated servers and
several game runtimes. An unresolved registration cannot activate a profile or
deliver game-scoped keyboard/mouse output until its executable is selected.

## Other clients investigated

These observations were made read-only on the development machine on
2026-10-22. Missing or stale local data does not establish what a client supports
on another installation.

| Client | Observed local evidence | Result |
| --- | --- | --- |
| GOG Galaxy | Neither `%ProgramData%/GOG.com` nor the conventional Galaxy program directory existed. | No installed-library format was validated. Add the game's executable manually. |
| EA app | `HKLM/SOFTWARE/EA Games` retained Battlefield 1, 2042 and V keys with empty/default values, without installation paths. EA Desktop program-data folders contained machine data and logs. | The inspected records could not supply trustworthy installed-game paths. No adapter is enabled. |
| Ubisoft Connect | The program-data launcher directory was empty; the inspected `Ubisoft/Launcher/Installs` registry location was absent. | No installed-library format was validated. No adapter is enabled. |
| Battle.net | The local Agent directory contained `product.db`, `.product.db`, `Agent.dat` and `aggregate.json`. The product databases were binary, without an implemented format reader. | Their presence alone does not identify launchable games. No adapter is enabled. |
| Xbox/Microsoft Store | No installed-game metadata adapter was investigated or implemented. | Use a directly accessible runtime executable where available. |

The local Epic records were Unreal Engine 5.7, Fab and Quixel Bridge, rather than
installed games. Their `CatalogNamespace=ue`, `bIsApplication=false` and
`AppCategories` engine/plugin values informed the exclusion rules. Epic game
discovery is covered by manifest fixtures; no installed Epic game was available
for a native launcher smoke test on this machine.

## Verification boundaries

Fixture coverage includes Steam secondary libraries, integrated-game identity
reuse, nested cached artwork, mismatched/duplicate manifest identities, path
traversal rejection, Epic runtime suggestions, incomplete installs and
engine/plugin/DLC filtering. Native Windows process smoke tests use private
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
