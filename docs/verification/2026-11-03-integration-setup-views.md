# Passive integration setup views

Partial #188 / #194 prerequisite. Setup decoration now reads prepared memory instead of inspecting files, checking removal state, starting jobs, or taking setup admission. Prepared views use the exact provider, executable, installation, compatibility and process scope plus the committed provider record. Cold or changed scopes cannot reuse a previous Ready result. Saved-settings errors and incompatible installations remain actionable without a source refresh.

Native source admission consumes a separate small configuration-proof cache. UI eviction or oversized display metadata cannot remove or manufacture authoritative readiness. Source readiness is conservatively withheld during setup workers. Full inspection still runs at its existing source cadence; this change does not claim notification-driven game acquisition.

Display limits are 519 scopes (512 authored titles plus seven built-ins), 256 KiB per view, and their bounded aggregate. Oversized display views report unavailable while retaining the independent source proof and original diagnostic state. Shared diagnostic handles keep cloning and JSON encoding outside cache locks. Full refresh removes retired scopes; cache ordering uses checked arithmetic and clears display entries on exhaustion.

Checking a different DCS folder produces a separate `checkPreview`. Its path, connection and result never qualify or overwrite the configured folder. The preview survives canonical source refresh for the same saved record and scope, and clears on another check or configuration change. The UI labels it “Last folder check.” Workers republish current progress immediately after a saved-record commit so a slow final inspection does not hide Cancel.

All seven settings writers use one writer-only gate. They compare a captured committed base before durable persistence, retain the repository's external revision check, and publish memory only after persistence succeeds. Passive readers never take this gate; their saved-record mutex is released before path validation, SQLite or filesystem access. Stale drafts and failed persistence preserve the previous committed view. SDK stamp reads similarly run outside the catalogue cache mutex. Cached UI vehicle metadata never falls back to loading catalogue files.

## Local verification

Final qualification passes 1,974 native tests with zero failures and 17 existing explicit ignores. All 148 native production-module coverage gates pass, including the setup cache, settings commit helper, catalogue capture and source inspection. The source/test stamp remains unchanged throughout qualification. All 123 games frontend tests, Svelte checking (zero errors/warnings) and the frontend production build pass. CI remains deferred until all features are implemented, per user instruction.

The final 51 focused native setup tests pass (one existing explicit ignore). Frontend coverage passes at 94.43% lines, 93.75% branches and 90.12% functions; all changed games helpers exceed their floors. Eleven source-coverage helper tests and nine integration-coverage helper tests pass.

The rebuilt Cockpit passes the private-folder regression: prepare B, configure broken A, check B, retain B's separate preview through the existing background refresh, then clear the preview on checking A. It restores the original provider settings. Evidence: `.research/reactive/game-setup-cache-native/1793702829892/evidence.json` and `cleanup.json`.

All six existing actual-app preview/source-wake/deadline/artwork regressions pass. Authored DCS setup/repair, independent BIOS/ownship processing and withdrawal/recovery pass (59.65 s); all four authored Assetto readers pass (71.95 s). Installation lists are restored, cleanup errors are empty, the original DCS provider revision is restored, and the profile is stopped. Evidence: `.research/reactive/setup-cache-running-regression.log`, `setup-cache-running-dcs.log`, `setup-cache-running-assetto.log` and `setup-cache-final-profile-state.json`. Tested executable SHA-256: `5bf48357c1f4d68bf8aa85689ca683b9278bbd9247ddc6166ded676c9be19fa5`, process 50372.

The first final native run found an existing overbroad metadata-read assertion: independent process observations settled from waiting to not-running between complete-view comparisons. No setup or metadata field differed. The corrected test compares the full committed metadata document and raw catalogue, unchanged admission epoch, and absence of a setup job. Its helper is test-only; production behavior is unchanged from the actual-app build above.

Final full native evidence: `.research/reactive/setup-cache-native-qualified/evidence.json`, `native-tests.log` and `integration-summary.json`. All 1,005 staged Rust/SQL/manifests/locks match the tested candidate after CRLF-to-LF normalization. All 142 unrelated modified files remain byte-identical. Proof: `.research/reactive/setup-cache-index-source-proof.json` and `setup-cache-unrelated-before.json`.

## Remaining scope

This does not complete the games read model or remove its UI/source polling. Full prepared game projections, immutable record/artwork paging, completion publication, the installed-car persistence lock, and UI subscriptions remain open. Source inspection and command admission still need their wider migration. Wider adapter/device/companion/MCP cutover and integrated qualification also remain open. These tests do not qualify physical input-to-preview/output latency or actual simulator/board effects; user-operated acceptance remains #199, first in progress. Assetto sampling approval remains unresolved.
