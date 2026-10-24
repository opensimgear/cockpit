# PR #168 CI follow-up, 24 October 2026

The initial cross-platform runs exposed platform assumptions outside the Windows acceptance:

- Recovery test fixtures now create temporary directories under a canonical temporary root. This avoids macOS /var aliases while retaining production rejection of symlinks/reparse points.
- Native IPC tests use the actual mock window URL rather than a Windows-only http origin. Production IPC authorization is unchanged.
- The game-installation symlink test asserts the actual platform-specific rejection, preserving the zero-write checks.
- Recording-mode admission diagnostics report only checks required for recording. Live-only enablement/qualification warnings no longer hide a transient recording resource conflict. A deterministic regression holds the manual-router lock and checks that exact rejection.
- The independent-clock test polls the asynchronous ordinary mapping projection within a bounded deadline rather than assuming one nonblocking status read is settled.

Local validation: complete mapping-host suite (113 tests) passes; mapping-host module coverage passes every 71% gate, with companion_runs 90.84%. Virtual-controller module coverage also passes every gate. Native MCP credential IPC, UI preference IPC, and game symlink regressions pass on Windows. Cross-platform CI validates the Unix-specific paths/origins.

At the user's explicit request, all jobs in companion-esp32.yml are temporarily skipped. The ESP32 investigation was stopped; no production ESP32 code was changed and no ESP32 acceptance claim is made. Re-enabling those job conditions restores their existing test/build definitions.

A later macOS job exposed one additional desktop-output fixture under the aliased temporary directory. That fixture and the remaining companion SQLite/TLS/cache fixtures now select canonical temporary roots too. The complete desktop-output suite passes (45 tests); companion-service/serial coverage again passes all production module gates. These changes affect test setup only.

The process-level native gateway test also now asserts that retired game cleanup is absent. Subsequent native macOS coverage reached older DCS/provider fixtures; those and remaining native temporary fixtures now use the existing canonical fixture helper. Full cargo test for src-tauri (library, binary, native_gateway_service integration and doc targets) passes locally. Slow instrumented storage could incidentally expire the mock backend lease in child-death/callback-deadline tests. Those two fault-specific fixtures now use an explicit fixed backend clock; real script deadlines and dedicated lease-expiry tests retain their real clocks. Mapping-host coverage passes again with all 113 tests.

The native module coverage runner now tracks companion/storage/panels.rs rather than its removed root-level file. A repository-source regression checks every default tracked production/test file before invoking Rust. All 43 native module coverage gates pass locally, including panels 85.71%; all six runner regressions pass. The cancellation-specific visibility test allows durable fixture Start to finish before testing its second guard rejection; only its test timeout changes, while production retains its three-second admission deadline.
