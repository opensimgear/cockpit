# Custom games and game artwork (#128)

Implementation was exercised in an isolated, running Windows native dev Cockpit
from `feat/custom-games-128`. Its Vite origin was `127.0.0.1:1421`, with a separate
application identifier and configuration directory. The existing main-checkout
Cockpit was left running. The native acceptance tests use real gateway IPC and
application persistence; no catalogue, process-watcher or IPC response is mocked.

## Observed native workflow

`tests/ui/custom-games-native.mjs` uses private owned GUI executables with the
same basename in two different directories. It verifies executable/name entry,
context-only game details, custom profile context save without a simulator
resource, manual Apply while closed, active-registration guards, exact-path
process observation, external Automatic selection, process exit, Cockpit launch,
and removal preserving saved profile references while rejecting a new missing
registration activation.

The native UI also selects built-in/custom artwork, saves focal positioning,
deletes the original image and reloads the application, then resets the override.
Actual installed Steam metadata is previewed and two selected entries imported;
reimport reuses the same registrations. The recorded imports were ARC Raiders
and Arma Reforger. The latter supplied cached Steam artwork. Those two import
observations alone do not establish successful game startup or gameplay.

The expanded final run passed in 91.8 seconds with no page errors. It also
verified Add/Cancel, rename and relocation preserving saved profile references,
missing-path edit rejection, invalid-image preservation, and shared-image
collection when built-in/custom overrides are reset. The executable-name
inference race found during initial testing was fixed and checked through ten
repeated form edits plus keyboard/name-first entry in the actual app.
Evidence and screenshots are retained locally under
`.research/custom-games-native/`.

`tests/ui/custom-games-steam-native.mjs` also passed against the running native
app in 40.0 seconds using the installed **CarX Drift Racing Online** (635260) and
**DiRT Rally 2.0** (690790). Both were launched through Steam, observed at their
selected exact runtime executable and activated with their matching saved
profile. Cleanup stopped only the newly observed PID/path/creation identities
owned by the test. Evidence is
`.research/custom-games-native/steam-final-context-guard-evidence.json`.
Hardware-first launch retains its immutable hardware owner's `gameId=null`;
the completed candidate and native child owner supply the game context. A blank
profile validates context/launch ownership, without claiming input delivery.
The actual active-context removal guard also rejected changes to each running
game, demonstrating that the context survives beyond the launch response label.

`tests/ui/custom-games-persistence-native.mjs` prepared a custom registration,
managed background/focal position and saved profile, then verified them after a
native restart from PID 63596 to PID 28876 with different creation epochs.
It relocated the executable through the actual **Use this path** control,
preserved the ID/artwork/profile eligibility, and removed the referenced game
through its confirmation dialog. The profile retained its unavailable reference.
Both phases passed; evidence is `restart-evidence.json` in the same directory.
An additional actual UI check verifies that the accepted path adopts native
canonicalization and hides **Use this path** after saving; its private
registration was removed. Evidence is `.research/custom-path-canonical-ui/`.

## Process and policy evidence

The standalone Windows process smoke uses the production discovery/activation
modules and private owned helpers. It verifies exact executable/PID/creation
identity, ambiguity, process exit/replacement, freshness and X-Plane's separate
exclusivity guard. Source hashes and executed binary hashes are recorded in
`.research/custom-games-128-process-smoke-final-20261022/`. This fixture is separate
from the actual Cockpit workflow and from installed-game observations.

Portable profile unit tests passed (137 tests). The unchanged 71% profile coverage
gate passed for all 16 modules; activation policy measured 100%, and the lowest
module measured 90.91%. Native focused tests cover custom-ID save/reapply,
Automatic/Manual ownership, launch admission/rollback, inspection/topology,
missing-registration recovery readability and absence of provider authority.
The larger-catalogue regression observes 256 registrations with one batched
process inventory per cycle and checks debounce, relocation, replacement and
removal revocation.

Frontend game-helper coverage exceeded the unchanged 71% thresholds. The
provider/navigation checks and affected frontend regressions passed. Native
bridge tests passed (11 tests), including bounded game catalogues containing
managed thumbnails and rejection beyond the dedicated response limit.

`pnpm check` reported no errors or warnings in Cockpit and the browser companion;
`pnpm ui:build` built both. The final game/runtime helper coverage was 95.30%
lines, 94.09% branches and 91.11% functions. The focused native instrumented run
passed 107 tests with unchanged source stamps; changed module line coverage was:

| Module | Coverage |
| --- | ---: |
| Game installations | 94.14% |
| Process discovery | 82.99% |
| Managed artwork | 86.44% |
| Library discovery | 89.20% |
| Metadata transactions | 97.08% |

Evidence is `.research/custom-games-native-coverage/`. The unchanged native
integration runner now also gates the three new production modules at 71%.
The focused suite includes 256-game batched observation, per-game process
inspection-error isolation, stale/busy transaction rejection, artwork staging
leases and Stop completing while metadata discovery is deliberately blocked.
Passive game status also bypasses metadata/profile admission without changing
control epochs; two held-lock regressions verify responsiveness and retain
invalid-client, oversized/unknown request and closed-runtime rejection.

## Launcher feasibility

The local Steam scan found 36 installed manifests across the available library.
Known redistributables are classified as tools; other manifest-only entries
remain explicit review candidates. Suggestions are not runtime identity proof.
The local Epic entries were Unreal Engine/Fab/Quixel and were excluded. Epic game
manifest discovery is fixture-tested; a real Epic game was unavailable here.
The [launcher matrix](../launcher-discovery.md) records the evidence and bounded
follow-ups for GOG, EA, Ubisoft and Battle.net.

Liftoff's Steam launch reached Steam's `ShowInterstitials` state and waited for
a user response. Cockpit's startup deadline expired without replaying launch or
granting game identity. This is retained as a failed startup observation, rather
than a successful game validation.

## Limits and existing failures

Hardware validation is deferred at the user's request and tracked in
[qualification #135](https://github.com/opensimgear/cockpit/issues/135).
No assembled USB/CAN rig,
physical controller input, controller hiding, virtual device output or gameplay
delivery is qualified by this feature run. The existing qualified backend gates
remain in force. Non-Windows native launch/process monitoring is not newly
qualified. Simulator regression fixtures are distinct from live DCS, Assetto and
X-Plane gameplay.

The initial CI run exposed an inherited `complete_rig` fixture mismatch, also
reproduced on untouched baseline `c144301`. The fixture now uses non-authoritative
native USB discovery to model disconnection and expects retained offline inputs
to be Unavailable; changed-contract inputs remain Blocked and token/graph
assertions remain intact. Production authoritative-removal behavior is unchanged.
The combined complete-rig and editor-metadata targets pass 4 + 9 tests.

Inherited validation maintenance also aligns the mapping-host lockfile with the
image dependencies already required by companion-service, updates the desktop
endpoint rejection assertion without changing its immutability check, and makes
the exporter budget fixture otherwise valid before testing the budget. Locked
offline mapping-host metadata resolves for Windows and macOS; its test library
builds. The local mapping-host suite passes 110/112 including permission retries;
the remaining two tests assume an unqualified driver backend, while this machine
has a qualified vJoy/HidHide setup. Qualification behavior is unchanged.
All 31 frontend checks from profile coverage through production build
pass, including 268 workspace tests. Export tests pass 4/4 with exporter coverage
96.83% lines, 95.59% branches and 92.86% functions.

The generic ESP32-S3 cross-build in the initial CI still selects 2 MB flash for
the existing 3.9 MB partition table. This feature changes no firmware, partition
table or flash configuration; the unrelated build failure is not reported as a
passing check. Wider CI results must be read for the current PR revision.

A 16-thread native library run was stopped after 1081 of 1095 tests reported
results because it exhausted host responsiveness alongside another checkout's
test process. Three deadline-sensitive failures passed when rerun individually
with one thread. This interrupted run is not reported as a passing full suite.

## Category and badge follow-up

Custom badges now appear on the right of game tiles and alongside the detail
page actions. Each built-in or custom game can have one optional category;
Add/Edit also includes category selection. Manage categories supports bounded,
distinct names, stable IDs, rename, deletion and Cancel. Renaming retains game
assignments; deletion makes affected games uncategorized. The games list filters
by All categories, Uncategorized or a configured category. Storage uses the
existing atomic native registry; stale category-list edits are rejected.

The rebuilt running native dev Cockpit passed `game-categories-native.mjs`:
category creation and duplicate rejection, Add/details assignment, built-in
assignment, right-hand badge positions, filtering, rename/reload durability,
delete/unassignment/filter reset and Cancel, with no page errors. Test-created
entries and category changes were cleaned up. Evidence is retained locally in
`.research/game-categories-native/`.

The 6 metadata and 8 installation tests passed, including category persistence,
rename/clear/remove, stale edits, invalid references/names and unchanged saved
state after rejection. Frontend checks report no errors or warnings; both
production builds pass. Game/runtime helper coverage passes at 95.47% lines,
94.50% branches and 92.00% functions. These follow-up checks are separate from
the earlier source-stamped native coverage run recorded above.

## URL images and crop popup follow-up

Tile backgrounds accept disk paths/native browsing or HTTP(S) URLs. Loading
produces a transient bounded JPEG preview; only Save background publishes the
image and crop together. Cancel leaves the existing override unchanged. Remote
loads use bounded reads, timeouts and redirects, validate each redirect scheme,
and apply the same format/decode limits as disk sources. Sources are not stored
as live remote image URLs.

The popup shows the complete source with a movable/resizable 16:9 tile frame,
Zoom, Fit image, keyboard controls and a live tile preview. Geometry clamps the
frame to the image at every size. The saved normalized crop is validated for
bounds and tile aspect ratio; rendering retains the image's aspect ratio. The
full managed image remains available for later repositioning without repeated
JPEG encoding. Legacy position metadata remains readable.

Final running native acceptance (`game-background-native.mjs`, isolated WebView
CDP 9223) passes in 6.36 seconds, with no page errors: portrait disk image,
pointer move/resize against image edges, URL download/redirect, landscape crop,
keyboard/zoom, Cancel, invalid-image preservation, source deletion, offline URL
reload, undistorted list tile and built-in image positioning/reset. Local evidence
and screenshots are in `.research/game-background-native/`. Temporary games and
test changes were cleaned up.

Native artwork tests pass 7/7; metadata and installation regressions pass 15/15.
Crop geometry helper coverage is 100% lines, branches and functions, including
portrait, landscape, square and extreme-aspect bounds. Frontend checks report
no errors or warnings, game/provider/launch suites pass, and both production
builds pass with the existing import/chunk-size notices. The final native review
build also succeeds. These checks supersede the initial build-space failure and
stale development/debugging sessions encountered during this follow-up.

## Inline game details and image form follow-up

The Installation card now contains the tile and Update image button on the left,
with one inline executable field, Browse and category controls on the right.
The Edit action is removed. Custom names save inline on Enter/blur and Escape
discards a draft; built-in names remain read-only. Name-only metadata updates
preserve the registered executable even if its file has moved or disappeared.

Update image opens one form with disk/URL loading and the current image's crop
editor inline. Reset to default sits beside From URL. Position current image
and the zoom slider are removed. All four corner handles resize the frame while
anchoring the opposite corner, keeping the tile aspect ratio and image bounds.

The rebuilt running native Cockpit passes the updated background acceptance in
5.73 seconds with no page errors, including all four pointer-driven handles,
disk/URL loading, cancellation, persistence, built-in reset, inline fields and
the shared card layout. Category assignment/configuration/filtering acceptance
also passes in 2.60 seconds. Screenshots and evidence are refreshed under
`.research/game-background-native/`. The metadata suite passes 8/8 (including
rename with a missing executable), frontend provider/launch/crop tests pass
20/20, checks report zero errors/warnings, and both frontend builds pass.
The native build succeeds after removing generated worktree build caches to
recover disk space. Hardware validation remains deferred.
