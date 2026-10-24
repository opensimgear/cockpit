# SQLite persistence (#140)

Implementation baseline: `d4808b1` (main, 23 October 2026). This includes Custom
games #128 and the compiled widget publication format. Work is isolated in
`feature/sqlite-140`; the unrelated widget/ESP32 checkout is untouched.

This is the inventory and implementation contract. Entries below describe the
required destination, not a claim that every owner has already been converted.
Acceptance evidence lives in `docs/verification/2026-10-24-sqlite.md`.

## Namespace and authority

One `cockpit.sqlite3` belongs to each Cockpit configuration namespace. The anchor
is the parent of `config_paths::config_path()`; `OSG_DCS_BIOS_CONFIG` continues
to choose an isolated namespace. An explicit `OSG_SQLITE_DATABASE` may choose
an absolute database path. No database path may contain parent traversal or
follow symlinks/reparse points. Database and backup files are private to the
current OS user. A native standalone Companion uses its own application data
directory and database, never a database on its remote Cockpit host.

The desktop app-config root and any configured native root resolve to the same
SQLite authority. A missing record starts from its validated default. Failed
reads or writes remain errors and never fall back to JSON, localStorage or
IndexedDB. Retired stores are ignored, including on the first launch.

The updated development instructions deliberately permit a clean start. There
are no legacy importers, migration receipts, schema upgrade chains or downgrade
compatibility. A database with a different layout requires a fresh development
namespace. Existing portable document and network protocol formats keep their
validators; they are independent of the database layout.

## Inventory

`records(domain,key)` stores validated JSON or native ciphertext as BLOBs, with
optimistic revisions and deletion tombstones. Profile graph, assignments and
ID/reference indexes commit in the same transaction. Assignment owners have
foreign keys; missing external device/game/publication targets remain readable.
The portable library retains its document bytes and semantic hashes.

| Source / durable writer | SQLite domain/key | Validator and owner | Verification |
| --- | --- | --- | --- |
| `profiles-v2/library.json`; `profiles::store::Store` | `profiles/library` | `Library::validate`, document and local-port contracts; S3 | Default/UUID/hash/revision/assignment roundtrip, CRUD and stale saves |
| `profile-resources-v1/inventory.json`; `resources_store::Store` | `resources/inventory` | `SavedRegistry::validate`; S3 | Offline restore, exact ports, no restored live owners |
| `controllers.json`; controller service `flush` | `controllers/identities` | existing saved-controller validator/bounds; S3 | Fingerprints, names, role, reassignment and Forget |
| `controller-visibility.json`; visibility settings | `controllers/visibility` | visibility `Config::validate`; S3 | Intent/revisions, concurrent edits, OS state remains observed |
| `profiles-activation-v1.json`; `profile_automatic_store` | `profiles/selection` | `Settings::validate`, rule UUID/priorities; S3 | Manual/default, automatic rules, preferred launch profile |
| `game-installations-v1.json`; `game_installations::Store` | `games/installations` | `Settings::validate`; S4 | Built-in/custom UUIDs, Steam/Epic source IDs, exact executable, categories |
| Same game envelope's backgrounds/default backgrounds | same transaction as game registrations | artwork validation/hash/path/crop; S4 | Edit/crop/reset/remove/import and restart; references survive missing images |
| `games.integration-setup.json`; `write_settings` | `games/setup` | connection/provider saved-record validators; S4 | DCS/AC/X-Plane settings/revisions, no implicit setup |
| `game-vehicles-v1.json`; game vehicles store | `games/vehicles` | saved catalogue/vehicle validation; S4 | Retained selection/seen vehicles, offline catalogue |
| `assetto-corsa-cars-v1.json`; AC car catalogue cache | `games/ac-cars` | current cache identity/version/bounds; S4 | Rescan, stable car IDs, stale observations |
| `desktop-input-v1.json`; desktop settings | `outputs/desktop-preferences` | `Preferences`/native process-identity validator; S4 | Enablement/scope/application references and stale saves |
| IndexedDB `opensimgear-cockpit-companion-designer`, `workspace/snapshot` | `companion/designer-workspace` | `validateWorkspace`, compiled publications/source/asset hashes; S5 | Draft/source/publication order/assets, 64 MiB/20 drafts/100 revisions |
| `widgets-v1.json`; `widget_library::persist` | `widgets/library` | compiled widget bundle/source validation; S5 | User library/update/import/hash preservation |
| `companions/companion-panels.json`; `companion_panels` | `companion/publications` | compiled publication validators, byte limits; S5 | Publish/archive/retention and interrupted publication |
| `companions/companion-panel-heads.json`; `save_heads` | same `companion/publications` transaction | panel/revision/hash head checks; S5 | Heads and publication/assignment references commit together |
| `companions/companion-listener.json`; host listener operations | `companion/listener` | listener interface/port validation; S5 | Enable/disable/listener restart without premature success |
| `companions/companion-firmware.json`; host firmware metadata | `companion/firmware` | board/profile/version validators; S5 | Firmware preference/assigned profiles and restart |
| `companion-host-state.dpapi`; `SealedSqliteStore` | `secrets/companion-host` ciphertext | native unseal then host record validation; S6 | Pairing/assignments/revoke/forget/reconnect; raw DB secret scan |
| `companion-identity.dpapi`; TLS native identity store | `secrets/companion-identity` ciphertext | unseal/identity certificate validation; S6 | Same host ID/CA fingerprint; locked/corrupt key diagnostics |
| `mcp-v1.json`; credential settings store | `security/mcp` | current settings/hash/scope validators; S6 | Real MCP client, port/enablement/revocation/concurrent revisions |
| Native Companion `companion-client.dpapi` | client DB `secrets/companion-client` ciphertext | existing native client validation; S6 | Native upgrade, reconnect and forget/re-pair |
| Native Companion IndexedDB `opensimgear-companion-publications` | client DB `companion-client/publications` | existing verified publication/cache bounds; S6 | 4-entry/24 MiB cache, selected panel/page, offline reopen |
| `osg-light`; `App.svelte` | `ui/theme` | boolean; S7 | Hydration before first render, cross-window/restart |
| `osg-devices-mode`; `SystemDevices.svelte` | `ui/devices-mode` | list/topology enum; S7 | Mode survives restart |
| `osg-canvas-navigation`; canvas navigation store | `ui/canvas-navigation` | wheel-action enum; S7 | Hydration, wheel controls, failed save |
| `osg-canvas-styling`; canvas styling store | `ui/canvas-styling` | arrow style, bounded radius, collapse boolean; S7 | Canvas appearance/restart, stale cross-window edits |
| `osg-system-inventory-v1`; `SystemInventoryRegistry::persist` | `inventory/remembered` | bounded components/tombstone validators; S7 | First/last seen, Forget tombstones, refreshed provider authority |
| App-config `desktop-preferences.json`; desktop `write_preferences` | `ui/tray` | `TrayPreferences`; S7 | Tray close/minimize, unavailable tray diagnostic |
| App-config `profile-progress-preferences.json`; progress settings | `ui/progress` | delivery and supported position enums; S7 | Progress position and restart |
| App-config `window-state.json`; window-state plugin | `ui/windows` | fixed main-window geometry/display validation; S7 | Coalesced move/resize, exit/restart/off-screen display removal |
| `profile-session-v1.json`; profile session journal | `recovery/profile-session` | existing manifest/phase/owner validation; S8 | Intent/effect/receipt faults, Stop/restart/owner lock |
| `profile-output-{session}-{generation}.json`; virtual output journal | `recovery/virtual/{owner}` | exact owner/generation/driver fingerprint validation; S8 | No output replay, neutral/cleanup receipts, second process |
| `profile-desktop-{session}-{generation}.json`; desktop coordinator journal | `recovery/desktop/{owner}` | desktop session/process receipt validation; S8 | Crash/foreground/Stop recovery, no injector replay |
| `controller-visibility-session.json`; visibility journal | `recovery/visibility` | existing session/owner/fingerprint validator; S8 | Explicit restoration only; lifetime OS lock retained |
| `games.recovery.json`, `games.abandoned-UUID.json`; game cleanup/archive | `recovery/games/{owner}` | current installation/process/recovery schema; S8 | Exit/crash/external modification and abandoned evidence |
| `setup-work/dcs-setup-journal.json`, `completed.json`, `restored.json` | `recovery/dcs-setup/{id}` | existing setup journal/hash/path validator; S8 | Intent and receipt boundaries; immutable backup bytes referenced |
| `mappings/output-recovery.json`, `mappings/companion-recovery/instance-<32hex>.json` | `recovery/retired/{owner}` | cleanup-only legacy receipt validator; S8 | Block activation until explicit safe cleanup; preserve unknown evidence |

Legacy `mappings/*.json`, `local-settings.json`, `companion-routes.json` and
old captured DCS profile authoring stores are retired. The old mapping crate
APIs remain for compatibility tests, never as native Cockpit configuration
authority. Their cleanup evidence is separately enumerated above. Restored
observations always begin offline/stale with no process, device or output owner.

## Explicit external/file exceptions

| Owner/data | Retained authority and reconciliation |
| --- | --- |
| OS keychain/Secret Service/Android Keystore | Protection keys stay in native service; only sealed BLOBs enter SQLite/backups. Locked/missing keys produce errors without regeneration over old identity. |
| Browser-only Companion bootstrap credentials/offline caches on other devices | Stay inside that browser's security origin; never represent Cockpit settings. No browser SQLite runtime. |
| Device flash/NVS/calibration and gateway discovery advertisement files | Owned by hardware/gateway processes; Cockpit stores intent/references only, observes fresh state and never replays ambiguous mutations. |
| Actual OS autostart, HidHide configuration, device access | OS/driver authority; record requested intent and checked receipts; display applied state only after OS verification. |
| Elevated vJoy provisioning registry/journal | Elevated provisioning authority remains independent; SQLite stores references/reconciliation evidence, never substitutes a database writer lock for driver ownership. |
| Game-side integration files and installer ownership markers | Managed with existing exact fingerprint/compensation checks; SQLite records desired settings and application lifecycle evidence. Game user files remain external. |
| Managed images, immutable archived widget/setup snapshots | Bytes may remain immutable files; SQLite owns mutable metadata, references, retention and GC intent. Stage/fsync bytes before reference commit; cleanup only after unreferenced verification. |
| Downloaded firmware archives, logs, generated exports, output audit traces | Reconstructible downloads or append-only diagnostics/user exports; not settings authority. Mutable firmware/listener selection metadata belongs in SQLite. |
| `.lock` files | OS lifetime locks only, no variable settings. Keep process/session ownership independent of SQLite transaction lifetime. |

## Service, durability and commit contracts

`osg-persistence` pins rusqlite `0.40.2` with bundled SQLite `3.53.2`
(`libsqlite3-sys 0.38.2` in the lockfile). Use the bundled build on all native
platforms. `application_id=0x4f534743`, STRICT tables, foreign keys on every
connection and bounded payloads are required. Foreign application IDs and
incompatible layouts fail closed; no upgrade or compatibility layer is provided. Corrupt content never becomes an
empty/default successful save.

Use rollback journal `DELETE`, `synchronous=FULL` and a 250 ms busy timeout.
Do not enable WAL by assuming APPDATA/XDG is local; redirected/network-backed
configuration requires qualified OS locking and a supported filesystem. SQLite
cannot guarantee storage-device/filesystem durability outside their contracts.
There is no WAL checkpoint to lose on normal shutdown. Online backup uses the
[SQLite Backup API](https://www.sqlite.org/backup.html); durability behavior is
defined by [SQLite pragmas](https://www.sqlite.org/pragma.html).

Database access runs on native command/storage workers, never in input/telemetry
callbacks. Immediate transactions check every expected revision before committing
related edits. Delete leaves a monotonically increasing tombstone to reject
delete/recreate stale edits. Keep transactions short: snapshot/validate assets and
observe OS effects outside the transaction. Update caches and broadcast changes
only after commit. A failed commit leaves the previous durable and visible
authoring state intact; live observations may still be displayed with diagnostics.

Profile graph and local assignments commit as one validated document. Resource,
selection and profile deletion/reassignment mutations share a multi-record commit
when their references change together. Companion publication/archive/head and
assignment changes similarly share a commit; sealed state must be prepared before
the transaction. UI/MCP/worker callers use the same repositories/revisions.

Recovery ordering remains intent commit, external effect, verified receipt
commit, and explicit compensation where needed. Never hold a transaction across
OS/USB/game/network waits. Startup loads recovery evidence before automatic
selection; initialization never starts output, launches games, un-hides controllers,
revokes trust or republishes panels. Lifetime locks and process/generation proofs
retain their existing semantics.

Restore is an application operation: stop/quiesce output and reconcile current
external effects, verify the backup database and a hash-checked asset
manifest, preserve current data as a rollback snapshot, restore through SQLite,
invalidate historical credential/session authority, then restart/hydrate. Historical
trust is not fresh proof: reissue/re-pair unless freshness is independently proven.
Never replay historical cleanup/output records. A raw file copy while a database
is live is unsupported. Backups retain OS-bound ciphertext portability limits.

## Delivery order and acceptance

S0 tracks this inventory; S1 supplies the shared service and the existing >=71%
production coverage gate per module. S2 is SQLite-only initialization and native
webview hydration under the revised development instructions; its original
legacy-import requirements are superseded. S3 profiles/controllers and S4 game
settings use the shared boundary. S5 designer/host, S6 protected/native-client
state, S7 UI preferences and S8 recovery complete the remaining owners. S9 audits
every writer, finishes backup/restore and runs assembled acceptance.

Each converted owner needs reopen, stale-edit and failed-commit tests. Final
verification includes a clean start, every settings category, restart, ignored
retired sources, second process/UI/MCP concurrency, interrupted activation,
Stop/recovery and backup/restore. Every manual feature also needs its MCP path.
Run native/frontend/build/package checks and all affected coverage gates before
pushing. Record Windows live evidence separately from unexecuted platform or
physical-device qualification. New modules stay below 500 lines and related
code lives in domain folders.

## Native designer workspace ownership

Desktop `CompiledWorkspaceStore` uses the typed `persistence_workspace` native
command. Browser previews explicitly use their separate IndexedDB adapter. A
native read/write failure stays visible; no desktop fallback reads IndexedDB.
Workspace drafts, source revisions and publication archives are validated before
commit. Source and parameter hashes must match the latest saved draft publication;
all publication assets pass the shared native validator. Sources and workspace
commit together at their exact SQLite revisions. Immutable sources added by MCP
or another process are merged into the acknowledged workspace and never erased
by an older designer snapshot. No file staging or asset garbage collection is
needed for these bounded inline BLOBs. Limits remain 64 MiB workspace, 20 drafts,
256 source revisions/8 MiB source library and 100 archived publication revisions.

Authenticated MCP exposes `designer_workspace_read` as hash-fenced 48 KiB byte
chunks. Begin/chunk/commit/abort tools upload a bounded UTF-8 JSON snapshot. Commit
rechecks native authorization and expectedRevision and uses the same owner as the
UI. Chunks are transient, client-bound and expire after ten minutes. The mutation
ledger makes exact operation retries idempotent; failed native commits preserve
the upload for explicit retry. Saving authoring data alone does not activate
outputs or assign a companion. Host publication/head/assignment authority remains
owned by the atomic host snapshot described below. Portal bulk Save and the fixed companion MCP controls use that same native boundary.

Listener settings and firmware observations now use SQLite. Missing metadata
uses validated defaults; corrupt or unreadable metadata remains an error. UI
and MCP listener changes carry settingsRevision, commit before applying the
listener effect and report configured intent separately from live bind state.
Listener effects serialize with acknowledged changes so an in-flight restart
cannot overtake a successful disable. Firmware captures keep their own exact
revision and retain metadata only for currently paired installations. Rendering
compatibility reads use the same SQLite firmware records. Retired files are
ignored. The host implementation is organized under `companion/host/`; every
new module stays below 500 lines.

## Encrypted native trust and MCP authority

`SealedSqliteStore` stores host trust, the TLS identity and native client
credentials under `secrets/companion-host`, `secrets/companion-identity` and
`secrets/companion-client`. Native OS adapters seal bytes before any SQLite
statement receives them. Windows DPAPI, macOS Keychain, Linux Secret Service and
Android Keystore retain their protection boundaries; external keys are not
exported. Android's native bridge seals/unseals bounded bytes and no longer owns
a SharedPreferences record. Browser companion credentials remain confined to
their browser origin. Retired `.dpapi` files are ignored.

Passive trust discovery does not create a namespace, identity or key. Existing
ciphertext must decrypt before a key-creating operation can run. Missing/locked
keys and corrupt records preserve the saved ciphertext. Exact SQLite revisions
reject stale writes and Forget operations; acknowledged revocation cannot be
overwritten by an earlier owner. Native public caches use the SQLite adapter described below. Host publication/head/assignment snapshots use the atomic owner described below.

MCP settings live at `security/mcp`. Only bearer-token hashes enter SQLite;
creation and atomic rotation return a token once to the requesting caller.
Native IPC and the authenticated MCP settings/credential tools share required
`expectedSettingsRevision` guards. Rotation replaces the old hash and metadata
in one transaction. Authorization refreshes SQLite state before admission,
including external scope changes, revocation and corrupt-authority failures.
No token is restored from the retired `mcp-v1.json` file. MCP operation results
and newly issued tokens remain bounded process memory, not durable logs.

The native MCP implementation is split under `mcp/server/` and `mcp/storage/`.
Frontend uncertainty requires reconciliation; it cannot fall back to a second
create/revoke sequence after an uncertain rotation result.

Native companion public content now uses `companion-client/publications` in
that application's own SQLite namespace, alongside its separately sealed
credential. The fixed `companion_cache_request` read/write API accepts exact
revisions and cannot address other domains or paths. One bounded snapshot holds
four panels at most, 24 MiB total and 13 MiB per entry, selected panel and last
page. Native client transport checks validate document/asset hashes; the
existing renderer revalidates the complete publication and capability identity
before opening it. Offline content never supplies authenticated input authority.

The native adapter never opens IndexedDB, including on a failed read or write.
Browser previews explicitly select their browser adapter. The private cache
document has no schema/version chain or importer. Corruption remains visible,
and explicit Clear uses the observed SQLite revision. A stale window cannot
resurrect a cleared cache. Panel/page selection is acknowledged only after a
successful durable write; clearing content does not remove sealed pairing.

## Atomic host content

`secrets/companion-host` and `companion/publications` commit together at their
exact revisions. The latter contains immutable publication uploads, inline
assets and explicit saved heads. Bounds remain 32 publications, 64 MiB admitted
publication bytes and an 80 MiB encoded archive. Assignment-only changes guard the current archive revision in the same writer
transaction without rewriting its bytes, so an acknowledged reference cannot
point to an archive changed since validation. Save/head/prune failures restore the previous
in-memory projection. Public content alone never supplies host identity or trust.

Native startup captures related records in one SQLite read snapshot, ends that
transaction, then unseals and validates all publications before registering
retained descriptors. Passive inventory, listener settings and empty authoring catalogues do not
initialize a host, TLS identity or listener. Frontend startup never republishes
workspace history or advances native saved heads.
Corrupt archives stay in SQLite and block their owner; there is no JSON fallback
or quarantine rename. The private host envelope is unversioned. This change uses
a fresh development namespace rather than upgrading an earlier checkpoint's
private record; portable publication/network formats remain unchanged.

Pruning retains assignments, profile baselines, live sessions and current saved
profile references read from SQLite. It does not trust a cached profile library.
If a profile changes during pruning, a read-dependency revision check rejects
the entire transaction without rewriting that profile's revision. No database
transaction spans sealing, source registration or transport waits.

Portal Save captures a native snapshotToken and submits one publication commit
with the complete direct companion selection. Archive, saved head and direct
assignments commit at once. All targets are validated before source effects;
stale or failed writes restore the prior snapshot and leave touched inputs
disarmed. Profile-owned assignments remain under journaled profile transitions.
Draft workspace saves are separate authoring commits; publication failure leaves
that editable draft available and does not acknowledge its deployment.

MCP exposes fixed typed companion inventory, pairing, rename/revoke, assignment,
retention, client-owned publication upload and native USB/firmware operations.
Write operations share native validation and the operation ledger. Record edits
require the observed snapshotToken; publication selection carries its own token.
USB status stays passive. Certificate export requires Configure access because
initializing a protected identity can be part of its native operation. Firmware
uses only the approved native release resolver and current device checks. No
arbitrary command, SQL, path or firmware bytes are accepted. Physical/mobile
qualification and the final S9 authority/restore audit remain separate gates.

## UI and desktop preferences

Native bootstrap reads seven typed UI owners before mounting either the main
portal or its progress window. The latter has a read-only bootstrap permission
and cannot call the portal gateway. Theme, device mode, canvas controls, tray,
progress and remembered inventory use exact SQLite revisions. A successful
write updates the native projection and emits its changed key/revision after
commit. Stale writes and uncertain acknowledgements require a fresh read.
Defaults never overwrite stored rows during bootstrap. Native failures remain
visible and never select browser storage. Browser previews use session memory.

Remembered inventory is bounded to 1 MiB, 1,024 components and 4,096 tombstones;
nested connection/reference budgets are validated. Persistence stores/returns
stale, unowned observations while retaining identity, first/last-seen history
and references. Providers establish current state again. Observation saves and
Forget serialize, reread shared metadata and await persistence. A second view
cannot resurrect a committed tombstone. Profile-bound records remain protected
by the existing cleanup review. No native device authority comes from these rows.

Tray and progress JSON writers and the window-state plugin are removed. Actual
autostart registration and tray/monitor availability remain observed OS state.
Tray intent updates the shared native cache used by window events. Main-window
move/resize signals have one pending slot; a bounded worker retains only the
latest geometry, commits off the window thread and flushes on close/shutdown.
It rejects foreign edits instead of rebasing old observations over them. Startup
clamps geometry to a current monitor work area and adjusts for DPI; minimized
or hidden state is never restored. Storage/placement failures remain diagnostics.

MCP ui_preferences_get/save uses the same fixed owners, validators and revisions.
Windows edits describe next-start geometry; observed native gestures establish
new current geometry. Remembered metadata is distinct from guarded device
Forget/revoke operations. No SQL, filesystem path or protected key is accepted.
