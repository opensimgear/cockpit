# SQLite later milestone history

Historical checkpoints; see [current acceptance](../2026-10-24-sqlite.md).

## S7 UI, inventory and desktop preference cutover

Theme, devices mode, canvas navigation/styling, remembered inventory/tombstones,
tray/progress and main-window geometry now share typed SQLite owners. Bootstrap
finishes before rendering, native commit notifications carry exact revisions,
and failed or stale saves remain explicit. The old progress client issuing
unrevisioned saves, the active localStorage preferences and desktop JSON/window
plugin writers are removed. Native progress code is organized under
src-tauri/src/desktop/progress; new domain sources stay below 500 lines.

Nine focused native UI tests, actual Tauri IPC guards and the native MCP owner
test pass. Coverage: UI repository 100%, validation 100%, geometry writer 96.97%,
bootstrap IPC 83.33% and bounded debounce 89.47% production lines. Evidence:
.research/sqlite-ui-preferences-final-coverage/. The full native regression is
green: 1,191 passed, zero failed, eight ignored (112.32s, four workers). The
initial highly concurrent run hit an existing USB timing assertion; its focused
rerun and complete bounded-concurrency rerun passed.

Frontend preference adapters reach 100% line/function coverage, with branch
coverage at least 93.33%. Canvas navigation/styling reach 100% lines/branches;
inventory/provider/topology gates pass. Profile workspace (278), canvas (154),
navigation (52), system inventory (93), companion/browser/transport suites and
UI render tests pass. Svelte/browser checks report zero errors/warnings; the
production UI build passes. Evidence: .research/sqlite/ui-*.

Actual dev Cockpit acceptance blocks localStorage, changes every visible UI
preference through its normal controls, forgets unbound remembered metadata,
resizes the real window, closes/minimizes to tray and reopens via single-instance
launch. Process 24832 restarted as 57284 with all preference values, the Forget
tombstone and exact physical geometry retained. Evidence:
.research/sqlite/ui-settings-restart.json, ui-settings-verified.json and
ui-settings-live-{prepare,verify}.log. A bound fixture was correctly blocked
by cleanup review before the unbound Forget case. Live checks also exposed an
empty-inventory normalization bug; the corrected default remains null, writes
no row and is asserted at both repository and Tauri IPC boundaries.

All four progress notification positions and moving an existing popup pass
against the actual native app, including Cancel and stale-operation protection
(2.64s). Evidence: .research/profile-progress-notifications-native/evidence.json.
Real MCP SDK/UI/restart acceptance passes (53.73s), including fixed UI settings
rows, native key/revision events, read scope and stale edits:
.research/mcp-native/1792822209914/evidence.json. The actual UI checks exercised
Windows desktop behavior; other desktop platforms and physical simulator/device
qualification remain separate. S8 journals and S9 backup/restore/final audit are
still outstanding, so this is not an all-SQLite completion claim.

## S8 profile, output and visibility journals

SQLite Evidence guards exact payloads and revisions in the same transaction.
Separate process-lifetime locks remain; busy/stale/corrupt evidence cannot
authorize a new effect or be overwritten. JSON journals are no longer read or
written. Private version/Pause migration fields are removed. Native and
standalone recovery discovery now query bounded SQL owner inventories.

Persistence (29), virtual controller (99), controller input (100), desktop output
(45) and mapping host (112) regression suites pass. Covered production lines:
Evidence 97.80%, output journal 99.10%,
visibility journal 95.30%, desktop journal 80%, profile manifest 94.57% and native
retained recovery 90.00%. Every module in the affected crate coverage gates
passes. The final instrumented native run also passes: 1,190 tests, zero failures,
eight ignored (470.76s, four workers). Native coverage now excludes complete inline test-only items while
retaining later production writers; 15 coverage-parser regressions pass.
Evidence: .research/sqlite/s8-*-coverage*.log and
.research/sqlite-s8-output-native-coverage/.

The actual dev Cockpit saves an inert profile to SQL, is forcibly terminated,
and restarts without replay. The exact active SQL manifest remains unchanged
until the visible Profiles Recover button completes cleanup. Status returns to
stopped, with no owner; the normal Delete dialog removes the fixture. Retired
JSON contents remain untouched. Evidence: .research/sqlite/recovery-native/
prepared.json, verified.json, recovery-required.png and recovered.png;
.research/sqlite/s8-recovery-native-{prepare,verify}.log. The final observed
restart process is 36604. No virtual controller, hiding or keyboard/mouse effect
was authored by this acceptance. Physical qualification is unchanged.

Per the fresh-start instruction, old cleanup/abandon archives will be ignored,
not migrated. New DCS setup receipts remain S8 work. Fresh development seeding
is required; normal restarts must preserve newly saved SQL data.
S9 backup/restore and the final authority/writer audit remain outstanding.

## Fresh-start and development seeding

The explicit instruction to initialize from zero removes old game cleanup/
archive support rather than migrating it. Its file writer, startup reader,
portal controls and old cleanup payload type are deleted. The private SQL
profile library is unversioned. Current portable formats keep their validators.

Debug startup atomically seeds Default, Dev · Signals, Dev · Workspace and the
generic Toggle source. Samples have no native assignments or protected
credentials and do not activate. Four native seed tests cover persisted rows/
indexes, restart edits, busy rollback, an unmarked existing library and corrupt
initialization evidence. Seed code reaches 100% production lines; current game
metadata reaches 96.43%. Profile store coverage is 98.44%, with all native profile
module gates passing (139 tests). The complete native suite passes: 1,186 tests,
zero failures, eight ignored (113.54s).

Frontend game coverage passes (games.ts 99.46% lines, 98.04% branches, 100%
functions), affected UI/workflow regressions pass (121), checks report zero
errors/warnings, and the production build passes. Evidence:
.research/sqlite/fresh-*.log and .research/sqlite-fresh-initialization-coverage/.

The running dev Cockpit now uses .research/sqlite/development-fresh/cockpit.sqlite3,
created from an empty directory. Actual editor acceptance verifies both visible
unassigned resource blocks and their connection. Renaming the saved sample,
restarting as process 14932 and reading it again proves it was not reseeded.
The test restores the sample label afterward. There are three profiles, one
widget source, no security/trust/recovery owner rows and no JSON files in that
namespace. Evidence: .research/sqlite/development-native/{prepared,verified}.json,
seeded-signals.png and seeded-restarted.png;
.research/sqlite/dev-seed-native-{prepare,verify}.log.

Remaining work is current DCS setup receipt storage and S9 backup/restore with
the final persistence/authority audit. No old-data migration is required.

## S8 current DCS receipt cutover

DCS setup uses `recovery-dcs` evidence for pending/completed/restored transitions and no mutable JSON journals. Exact revision/byte guards precede filesystem effects and compensation. Ownership locks survive the complete operation and release on process exit. Immutable before-script bytes keep checksum/path checks. Old JSON is ignored.

The 39 focused native tests pass (one network archive test initially skipped), including busy rollback, stale/equal-byte-new-revision edits, malformed/bounded records, process-exit recovery, interruption, external edits, and reverse removal. Coverage with both checksum-pinned official archive smokes passes: main setup 84.55%, aircraft 96.41%, physics 94.44%, SQL ledger 95.81%, record validation 97.56%. Evidence: `.research/sqlite-dcs-receipts-coverage/` and `.research/sqlite/s8-dcs-receipts-coverage.log`.

The complete native suite passed: 1,191 tests, zero failures, eight explicitly ignored hardware/browser/archive proofs (110.41 s). The rebuilt running Windows dev Cockpit passed `tests/ui/sqlite-dcs-receipts-native.mjs`: reused the approved module from a pinned archive in a private user directory, set up the current scripts, inspected completed SQL receipts/no JSON receipt files, removed the installation through native IPC, verified exact original export bytes, preserved the external module and recorded a restored SQL receipt. Evidence: `.research/sqlite/dcs-receipts-native-acceptance.log` and `.research/sqlite/dcs-native/*/verified.json`. The native rebuild caught and corrected an explicit filesystem import needed outside test builds.

## S9 backup/restore implementation and pre-integration acceptance

The shared database now pins a random incarnation, rejects replacement/restore
from old handles, shares that pin across registered roots, and never recreates
a deleted database through an existing handle. Restore is one checked SQL
transaction rebuilding relational profile indexes. Historical secrets, MCP
security, client caches, automatic selection/listener intent and cleanup authority
are excluded. Public panel archives receive a newly sealed empty host identity
in the same transaction. Windows DPAPI tests verify saved compiled panels and
heads remain, the host changes and previous pairings disappear. A native namespace
lease spans the application lifetime. Restore validates quiesced recovery bytes
across restart, first commits a rollback backup, and never replays external output.

The portal's General settings and five typed MCP storage tools share the backup
catalogue, revisions, bounds and stopped-owner admission. Read credentials cannot
mutate backups. Immutable bundles contain an online SQLite snapshot and hashed
referenced artwork/DCS before-script assets; mutable catalogue/intent metadata
is in SQLite. Bad hashes, missing assets, busy/stale commits and changed recovery
records preserve current data. Repeated stopped journal saves are idempotent;
foreign revisions still reject them. Native shutdown releases its namespace lease
before process relaunch.

Actual Windows acceptance in a fresh seeded namespace passed
`tests/ui/sqlite-backup-native.mjs` with populated compiled panels. It exercised
portal Create/Restore/Restart, real SDK MCP create/remove/stale saves/read-scope
rejection, a changed profile revision, a full process exit and restart, rollback
retention, a new database epoch and host ID, retained publications and revoked
old MCP credentials. No profile/output was activated. Evidence:
`.research/sqlite/s9-backup-populated-native.log`,
`.research/sqlite/backup-native/verified.json` and its screenshots. The assembled
real MCP/UI preference/normal restart suite also passed:
`.research/sqlite/s9-mcp-assembled-native.log`.

Pre-integration module gates pass: backup catalogue 93.85%, manifest 94.03%,
assets 91.45%, startup 94.25%, admission 91.36%, restored-host preparation 100%,
profile manifest 94.62%. Core restore is 77.02% with all core modules above 71%.
All companion service gates pass, including host_content 100% and sealed SQLite
100%. Reports: `.research/sqlite-s9-restored-panels-coverage/`,
`.research/sqlite-s9-restored-host-core-coverage/` and
`.research/sqlite-s9-companion-service-coverage/`. All 41 frontend CI commands
passed, including checks/build and each coverage gate. Eleven native crate suites
passed. The native mobile Companion intentionally refuses a Windows target;
Android/iOS build and mobile IPC qualification require their SDKs and were not
bypassed. The complete Windows native suite before the final fresh-host addition
passed 1,202 tests, zero failures and eight explicitly ignored proofs; the final
complete rerun and incoming main integration are recorded below when finished.

The final audit discovered current main advanced to
`b262567a3e5f526686eedaeee35258f381d96803` with eleven widget/UI commits. This
checkpoint is not the final S9 cutover: integrate that work, resolve the portable
source fixture drift found by the fixture check, then run final acceptance before
pushing the completed milestone. No existing user configuration is imported.
