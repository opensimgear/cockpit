# Game context observation ordering — 18 October 2026

The macOS native job `117520745349` for `9a7de802` failed the child launch-owner test at `stage_launch_context`: “The verified game preparation changed; acquire a fresh process/setup/catalogue proof.” The job retained 788 passing tests, one failure and four explicit ignores. Its raw log is `.research/p9-final-9a7de802/macos-native-job.log` (SHA-256 `67db9230a1e1830899b9231938d92306d0389bfe5af424cb8d7201023e0e7857`).

The launch fixture starts the native game observer and then saves X-Plane provider settings. An observer that captured `setup-required` before that save could finish after a synchronous preparation had captured the configured process and catalogue. Publishing the older observation retired the newer preparation. The guard rejected it correctly, but this ordering could also interrupt an ordinary Save followed by preparation. This was a production liveness race, not evidence that a longer launch deadline was needed.

The broker now reserves an observation ticket before metadata, settings, catalogue or process reads. Each game accepts a completion only once and only after its latest accepted ticket. Direct invalidation advances that fence; counter exhaustion withdraws proof and context authority. Process identity, provider/catalogue validation, owner matching and launch deadlines retain their existing checks. No preparation or command retry was added.

The launch test retains its original settings-save initialization. A new gated regression holds the real background observer after it captures the old setup state, saves settings and obtains a valid preparation, then releases the old completion. It holds the following background scan before publication, so the assertion sees that exact ordering. The current preparation must remain byte-for-byte equal and valid; no game context can be obtained without an applied owner. The injected platform rejects launches, and the output callback panics if invoked.

## Local evidence

The deterministic pre-fence witness retained the older completion invalidating the new proof with the same error as CI. Its source/artifact record is `.research/p9-launch-fixture-race/before-fence-frozen.json`; it demonstrates the causal ordering, not a reproduction of the macOS scheduler. The temporary preseeded fixture from that investigation is not part of the final change.

The final normal Windows test executable was copied from the actual Cargo JSON artifact, SHA-256 `5751d0e5407446aa5968b2f2839df60516fd28c600c1b5b6e14db25292d746cf`. The 385 captured source hashes remained unchanged during build and focused tests; the manifest reports no changed sources.

| Actual filter (`--test-threads=4`) | Result |
| --- | --- |
| `game_context::tests` | 12 passed |
| `games::context_runtime::tests` | 7 passed |
| `profile_session_host::launch::tests` | 12 passed |
| `games::launch::tests` | 13 passed |

All 44 focused tests passed. The broker tests also cover completion order, explicit invalidation and exhausted tickets; existing context tests retain settings-save invalidation, process replacement and owner/generation fencing.

Retained build JSON, copied executable, source manifest and four test logs are under `.research/p9-game-observation-fence/after/`.

| Final source | SHA-256 |
| --- | --- |
| `src-tauri/src/game_context.rs` | `2153d4f786416be272e8c1d9314e23e6359200b9cb37e426937ebcd09eb335e2` |
| `src-tauri/src/game_context_runtime.rs` | `ccafcde005c7046925170082b242f83f87a7bd5d691b6d59a6c7aa31fc1dc6f6` |
| `src-tauri/src/profile_session_launch_host_tests.rs` | `1fb019f6311c93334e5eb1cbfb1f32d2593ab0b41c4308dabde7b4297955f69f` |

These are native broker/runtime/Host tests with injected process observation. No Manager, live service, game process, controller acquisition, driver write or simulator command was used. They do not establish actual game or physical rig qualification, and the local pass does not substitute for a new macOS CI result.
