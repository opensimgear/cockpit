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

The final local-first audit found additional fixture and harness gaps before the next push:

- Injected vehicle selections now remain under explicit fixture control. The real two-second telemetry freshness policy is unchanged, and a regression ages the injected observation before polling both tracker paths.
- The native companion wire fixture retains received feedback deltas rather than overwriting a consumed value with an empty subsequent acknowledgement. The original one-second assertion deadline remains unchanged.
- Windows native CI builds the browser companion before Rust compilation, matching the local embedded browser path. Upload validation also checks that unknown fields and non-boolean saveAsLatest are rejected before initializing services or retaining an upload.
- The action coverage report reads and writes UTF-8 explicitly, avoiding Windows code-page failures on annotated Unicode source.
- Production C device acceptance reads the exact recovery-profile SQLite payload through a read-only connection. Forced death, watchdog restoration, cold startup, rejected Apply and explicit Recover retain their exact-evidence assertions; the obsolete JSON manifest must remain absent. Game acceptance asserts the retired cleanup field is absent.

The macOS logic prototype job passed an isolated rerun without changing its production five-millisecond evaluator budget.

Final pre-push local matrix (all applicable checks pass):

| Suite | Result |
| --- | --- |
| Frontend CI matrix | Windows: 41 commands plus clean Svelte check; Linux/Node 22: all 42 commands, including check and production builds |
| Native Cockpit | Windows: 1,203 library tests; Linux: 1,205 library tests; binary/service integration and doc targets pass on both |
| Portable native crates | All 11 crate suites pass |
| Coverage | Persistence, profiles, controllers, virtual output, mapping host, desktop output, logic, mapping, actions, DCS, integration, setup, companion and native module gates pass |
| Native module coverage | All 43 modules pass; minimum 74.29% |
| Companion coverage | All 39 modules pass; minimum 72.18% (commands) |
| Python and prototype checks | 121 script tests, 17 controller-mapping tests, 14 qualification tests, eight Unix bootstrap tests, 26 coverage-runner regressions and six Rhai prototype tests pass |
| Independent production C simulations | All six scenarios pass after replacing obsolete file/field assertions; active SIGKILL/watchdog/cold recovery verifies exact SQL evidence |
| Pinned DCS runtime | Both archive smoke tests and Lua checks pass; 188 exact control dispatches |
| Native acceptance boundaries | Windows driver-present and Linux namespace-isolated absent-uinput checks pass; companion mapping/feedback WSS acceptance passes |
| Windows packaging/probes | Development package, both C# probe builds, recovery probe's eight assertions and PowerShell parsing pass |
| Running dev Cockpit | Actual IPC health verifies SQLite 3.53.2, DELETE/FULL, 250 ms busy timeout and the dedicated feature database |

Local logs and summaries are retained under .research/sqlite, .research/local-final-*-coverage and .research/local-linux-*-coverage. Initial failures remain in their diagnostic logs; the final device acceptance run has six passes and zero failures. The Windows action report's code-page error was repaired and the final Linux action gate passes. An earlier Windows DCS measurement was invalidated by a fixture edit; the frozen final Linux DCS and setup measurements pass.

ESP32 jobs remain explicitly deferred by user request. Mobile native SDK builds, macOS-native behavior and physical game/device qualification require their respective environments; local results do not claim those qualifications.
