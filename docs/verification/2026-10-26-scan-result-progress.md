# Scan result progress — 2026-10-26

Library enumeration previously set every library to `complete` before global
identity matching and serial thumbnail preparation published any result rows.
The native cockpit reproduced a 7,752 ms interval between complete library
labels and the first results: 6,816 ms versus 14,568 ms from scan start.

Enumerated libraries now remain `preparing` until the review is ready. Matching
publishes stable result rows before artwork preparation; image updates arrive
progressively without holding the discovery mutex during decoding. The UI
distinguishes scanning, matching and artwork preparation. Apply remains guarded
until readiness. Late artwork cannot replace another scan's rows.

Discovery thumbnails use a bounded 256-entry in-memory LRU cache keyed by source
path, modification time and byte length. Replaced/deleted sources are checked
before a cache hit. Image dimensions, quality and source/response bounds remain
unchanged.

Actual native WebView checks used real Rescan actions and native requests. First
result rows were available at 7,923 ms (visible in the UI at 8,481 ms); artwork
finished at 16,442 ms. No completed-library/empty-result frame was observed.
The next scan finished at 7,954 ms using cached artwork. Row IDs remained stable
through artwork updates. Timings are local dev-build observations, not a general
performance guarantee. No library changes were applied and no game was launched.

Focused progress rendering and blocked-artwork concurrency tests passed. The
native suite passed 1,290 tests with nine environment-dependent tests ignored.
Discovery completion coverage measured 96.67%, thumbnail cache 100%, and artwork
sources 89.23%; all measured discovery modules passed the 71% gate. Svelte
validation passed with no errors or warnings. Detailed evidence is retained in
`.research/scan-delay/`, including `before.json`, `after.json`, `after.png`, test
logs and coverage reports.
