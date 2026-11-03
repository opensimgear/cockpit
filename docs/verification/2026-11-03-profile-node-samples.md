# Applied-node sample stream verification

This is a partial #188 / #194 / #196 milestone. It replaces the applied virtual-controller monitor's 500 ms status/inspection loop with shared exact-owner node subscriptions. It does not qualify physical input-to-screen or raw-input-to-virtual latency, simulator effects, whole-app migration, or MCP streaming transport.

## Contract and ownership

`profiles.samples.<sha256>` hashes the JSON array `[session,generation,profileId,profileRevision,gameId,nodeId]`. The snapshot contains the immutable owner, node ID, applied snapshot and program hash, plus endpoint/direction/contract/value-quality samples. It deliberately omits manual inspection's per-port `observedUs`; repeated source liveness cannot manufacture preview changes. The envelope clock is `profile-samples:<native controller admission clock UUID>`, and captured time records the reducer's last exposed value/quality change, not transport receipt or hardware acknowledgement.

Static node declarations and source hashes are prepared outside authority and shared across bindings. Preview preparation failure is cached independently of routing activation. Subscription binds a pin under the exact active coordinator owner, releases authority before baseline waiting, and starts a separate lazy worker. Routing only updates a checked value-change revision and posts an atomic wake; it does not encode snapshots, invoke consumers or wait for them. The worker copies bounded values under brief coordinator guards, releasing between nodes, then encodes/publishes off authority. No preview polling timer is installed.

Limits: 32 node topics, 1,024 total retained ports, 128 ports per node, 64 simultaneous subscription pins per topic, and a 1 MiB minus 2 KiB capture/encoded-body ceiling. Revision exhaustion disables observation while routing remains independent. Admission or encoding failure withdraws the preview rather than faulting routing. Stop/replacement retire exact passive topics without retaining source readers, sockets or output authority.

Latest-state delivery is explicitly limited to complete preview snapshots. A slow consumer retains its current in-flight frame and one pending replacement rather than an obsolete state backlog. Ordered input/event/action publication keeps its existing path; real loss/generation boundaries survive coalescing. Frontend consumers share one exact owner/node subscription, validate full bounded payloads, isolate listeners, and require a fresh baseline after loss. Visibility, inactivity, cleanup and owner changes detach subscriptions. Missing live values display unavailable rather than a numeric fallback.

## Verification

- Portable streams: 57 tests and 14 production-module line-coverage gates pass; new latest-state module has 100% production line coverage.
- Portable mapping: 197 tests and 35 production-module line-coverage gates pass. Controller: 147 tests / 30 gates; mapping-host: 123 tests / 12 gates; all pass.
- Native sample schema: seven focused tests pass, including matching native/frontend hash vector, false/zero/quality values, exact identity, allocation/encoding bounds and passive captures.
- Native worker: seven focused tests pass, including lazy/idle behavior, slow consumers, blocked-capture retirement, pin/topic/port/counter limits, failures and shutdown waiters.
- Frontend: 858 isolated profile tests pass serially; seven focused sample tests pass, including unchanged-owner status updates, visibility/owner cleanup and late/reentrant subscription fencing. Svelte check reports zero errors/warnings and production Vite build passes.
- Changed sample frontend coverage: 96.39% lines, 83.10% branches and 91.30% functions (71% floors); individual model/shared modules exceed each floor.
- Full native regression: 1,940 pass, zero failures, 17 explicit existing ignores; all 137 production-module coverage gates pass. New sample runtime modules cover 96.55–98.44% production lines; the gateway helper has 100%. The source/test stamp is unchanged throughout the successful run.

## Actual running Cockpit

The task-owned native executable was rebuilt from the isolated candidate and launched against the same candidate frontend. Final frozen-source SHA-256: `36bfcf3aa6ebe547b2844f8fb988843d86bf63e555770cb43155f25a5e9555e4`; process at final verification: 65128. The earlier build (971acb2 / 46556) additionally exercised the game regressions; the final build removes an unused gateway helper and adds integration tests, with no routing behavior change. The Vite candidate server was restarted to clear module cache after source export (its research-directory watcher excludes incremental changes).

`OSG_PROFILE_SAMPLES_NATIVE=1 node --test tests/ui/profile-samples-native.mjs` passes against the actual Tauri WebView. A sinkless DebugController → Identity fixture has no virtual or desktop outputs. Two shared frontend observers receive the initial known-false baseline without an explicit input, follow true and false native debug input, withdraw on Stop, reject reopening the retired owner, and receive a fresh baseline after restart; old-owner input is rejected. Its own profile and subscriptions are cleaned up. Evidence: `.research/reactive/profile-samples-native/1793695139219/evidence.json`.

This check exercises actual native transport and mapping with authored input. It does not measure physical report capture, screen presentation or a virtual-controller driver. Those remain explicitly unqualified in #199, first in the GitHub progress lane. Assetto sampling approval is also unresolved: the user requested a clearer explanation, not approval of a sampling exception.

The rebuilt Cockpit also passes existing authored pulse/script deadlines and independent source baselines, plus real sinkless physical-reader activation/replacement/Stop. The all-four Assetto fixture passes (86.89 s), and the DCS native fixture passes (32.95 s); their installation lists are restored and cleanup errors are empty. Evidence: `.research/assetto-capture-native/1793694700793/evidence.json` and `.research/dcs-physics-native/1793694847024/evidence.json`. These are authored telemetry checks, not simulator-confirmed effects or physical latency measurements.

Validation corrections: the initial broad frontend run contended with builds and tripped graph timing assertions; the final serial isolated run passes all 858 tests. The first full native run exposed a test assertion that wrongly required a later revision even when its value was intentionally coalesced; it now waits for the final value. An existing optional-provider cache test failed under load and passed its isolated recheck. No unrelated production cache change was made. The successful frozen full-suite rerun above is authoritative. Native evidence is `.research/reactive/profile-samples-native-coverage/evidence.json` and `integration-summary.json`.

The staged native source proof matches all **983 Rust/SQL/manifests/locks** against the tested candidate after normalizing CRLF to LF (766 files differ only in line endings). Evidence: `.research/reactive/profile-samples-index-source-proof.json`.
