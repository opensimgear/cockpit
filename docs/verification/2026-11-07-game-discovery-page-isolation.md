# Game discovery page isolation, 2026-11-07

The native gateway baseline found discovery traffic while Devices was open.
Two runs with the committed `7b7112cd` frontend and the same Windows native
executable each recorded 181 game-page reads over 60 seconds. A loopback
comparison server verified the frontend identity without changing the existing
dev server. The second trace captured 181 successful responses and reconstructed
36 distinct complete page documents. Each ID and offset was read once. The
documents contained changing scan entries and observations.

## Change and verification

The native game read-model worker publishes the scan review through
`games.discovery`. Catalogue subscribers keep `games.state` and its independent
revision. The Games view subscribes to the scan review while mounted. It keeps
the last complete review visible during a page replacement and pauses writes
until the current review is ready. MCP can subscribe to the same review and
resolve its existing immutable page references.

The rebuilt native dev Cockpit ran with a private SQLite database and WebView
directory and the isolated candidate frontend. In 60.005 s on Devices it
issued 14 app gateway POSTs, with zero game-page reads and zero app profile
status reads. The session reported stopped with no owner or fault at both
endpoints. After opening Games, 10 successful page reads loaded the scan
review with two library sections and an enabled Apply control. The check left
the proposed changes unapplied. Candidate executable SHA-256 was
`35bb7380f75bff2bf45c31b40acf1d32594334fe733dca1143c728133227215e`.
The traces are in `.research/reactive/r0-baseline-clean/` locally.

The Windows suite passed 2,391 tests, with 18 ignored. The native regression
covers catalogue isolation, page availability, reopening the review and
shutdown retirement. Changed production line coverage was 94.92% for the
read-model runtime and 100% for MCP observation URI admission. LLVM function
coverage was 70.21% and 100% respectively. The isolated native manifest
requested only the Rust library artifact to save disk space during local
testing. The repository manifest and packaging configuration were unchanged.
The Windows dev executable and production frontend built successfully.

All 73 focused frontend tests passed. The shared game observer reached 92.97%
lines, 91.14% branches and 72.73% functions. Model validation reached 89.72%,
78.57% and 77.27%. Svelte reported no errors or warnings.

This measures avoided WebView work during discovery. Controller motion-to-DOM
and raw-input-to-virtual-device latency are separate physical checks in #199.
The representative concurrent workload and R0 budget qualification remain open.
