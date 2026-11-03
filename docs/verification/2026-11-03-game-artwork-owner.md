# Game artwork owner and passive Steam cache

Partial #188 / #194 prerequisite. Game status previously queued optional Steam downloads and retried failures while decorating a read response. That work now belongs to startup, committed library metadata and completed discovery preparation. This is not the complete games read-model migration: local artwork/setup inspection and game-state UI polling remain open.

## Ownership and bounded work

The Steam decorator reads prepared memory only. It never queues work, reads retry time, starts workers or performs file/network I/O. Cached images use `Arc<str>`; the service lock copies only shared handles and small bookkeeping, and JSON strings are built after unlocking. The owner records accepted pending intent before its scheduler runs, so a response cannot falsely report completion and make the existing UI stop waiting.

A `NativeWake` scheduler parks until targets change, an actual download completes or the next real five-minute retry deadline expires. There is no state-refresh timer. Source registration stores exact app IDs and scan identity, not a full JSON snapshot. Bounds: eight owner registrations, 1,536 deduplicated app IDs per owner (512 library plus 1,024 discovery selection limit), 128-byte scan identities, 512 globally shared cache states and four existing HTTP workers. Existing 128 KiB JPEG preview, HTTP body/timeout and disk-cache bounds remain enforced. Explicit refresh shares cache admission and cannot insert a 513th state; successful disk refresh still returns its path if memory admission is full.

Per-owner attempt records prevent evicted Ready entries from creating a repeated-download loop. Monotonic tickets reject old queued/in-flight results after target removal/replacement. Closing an owner cancels orphan queued work and joins its scheduler without joining HTTP. Ready immutable image buffers may be shared across owners. In-flight HTTP can still finish an existing app-ID disk-cache write after retirement; its late result cannot change the current in-memory view. That disk-cache qualification remains explicit for the wider games source migration.

Metadata preparation resolves higher-priority local artwork outside source/admission locks, then fences the target replacement against the saved-library revision and exact discovery scan/status/revision. Post-commit artwork preparation runs only after the Gateway has released both profile and game admission. A deterministic regression blocks this actual preparation stage while requiring Profile Stop to acknowledge within a 250 ms test guard. The guard is a regression ceiling, not a physical input or Stop latency measurement.

## Local verification

- Final native suite: 1,959 pass, zero failures, 17 existing explicit ignores.
- All 139 native production-module coverage gates pass, with an unchanged source/test stamp; new owner module has 93.38% production line coverage.
- All 30 focused Steam tests pass, with one explicit live-network ignore in that invocation. The existing live Steam download/cache/offline test is separately enabled and passes.
- 121 isolated games frontend tests and all 37 local coverage-helper regression tests pass.
- Two native DevTools target-selection tests pass. The actual-app harness now selects only the main entry, preventing a still-open restricted activation window from being used as the command surface.
- Final native production build passes. Executable SHA-256: `8a425bb69f222ccaf58f33c3a9644b95c18720f92652226a8315885eae8f785d`; actual verification process: 43260.

Actual rebuilt Cockpit: an owned, never-executed metadata fixture is registered with local artwork; twelve reads preserve it; position replacement, reset and removal pass; the native profile remains stopped. Preview sharing/retirement, authored pulse/script deadlines, independent source baselines and sinkless physical-reader activation/replacement/Stop also pass (six actual-app checks). Evidence: `.research/reactive/game-cache-native/1793698075617/evidence.json` and `.research/reactive/games-cache-running-final.log`.

The first actual-app pass after rebuilding hit the existing DevTools selector race: it selected the activation popup and received its legitimate permission rejection, then a closing-target timeout. The selector now excludes the actual `profile-activation-window` query parameter, and its focused regression tests cover that URL. The complete six-check rerun passes; a further native run with protocol/evaluation error recovery passes the harness coverage floors (97.87% lines, 84.38% branches, 92.86% functions). Application permissions were unchanged. Earlier review also caught the pending-receipt race, terminal-cache cap bypass and outer-admission I/O; the final rerun includes all corrections.

Final DCS (56.14 s) and all-four authored Assetto (74.26 s) actual-Cockpit regressions pass. Installation lists are restored and cleanup errors are empty. These fixtures never establish physical latency or simulator-confirmed effects. Physical/board/simulator acceptance stays in #199 at the top of progress. Assetto shared-memory sampling approval remains unresolved. Per user instruction, CI work is deferred until all features are implemented; this milestone is qualified locally.

Full native coverage/source-stamp evidence: `.research/reactive/games-cache-native-final/evidence.json` and `integration-summary.json`. All 987 staged Rust/SQL/manifests/locks match the tested candidate after CRLF-to-LF normalization (761 files differ only in line endings). All 141 unrelated dirty files remain byte-identical. Proof: `.research/reactive/games-cache-index-source-proof.json` and `games-cache-unrelated-before.json`.

DCS final evidence: `.research/dcs-physics-native/1793698166766/evidence.json`; all-four Assetto evidence: `.research/assetto-capture-native/1793698223084/evidence.json`.
