# Complete portable profile rig

This is fixture evidence from the real resource registry, profile compiler and typed mapping reducer. It does not verify vJoy, HidHide, a connected device, a running game or a companion display. The captured backend uses `BackendCapabilities::default()`, so native activation and hiding remain blocked by qualification.

The repository scenario is `tests/fixtures/profiles/complete-rig.json`. Its replay and independently specified expected samples/requests are in `complete-rig-replay.json`. The replay pins the document SHA-256 with checkout line endings normalized to LF. The integration test is `crates/profiles/tests/complete_rig.rs`.

The graph contains:

- Third-party A and B feeding one named virtual controller, plus one actual companion control.
- Multiple OSG modules feeding a second named virtual controller.
- Third-party C with a hide-while-active intent and no invented signal route.
- A separate OSG event source feeding an explicit atomic DCS command invocation.
- DCS altitude in feet feeding visible conversion, hysteresis, conditional display and formatting nodes. The resulting state feeds a companion warning widget and a text widget; a separate visible setter requests a physical indicator change.

Both CAN-only and direct USB variants run the same portable graph. The real registry correlates the USB and CAN observations into one resource, keeps both connection attributes and selects the authoritative USB input. Gateway, serial endpoint and route metadata never become portable graph edges or exports.

The replay checks exact computed controller states, raw `10000 ft`, converted `3048 m`, conditional warning state and formatted `ALT 3048.0 m`. It compares complete requested feedback and physical setter tuples. DCS requests are zero-argument atomic one-shot tuples; none is interpreted as a device or game acknowledgement.

Additional assertions cover native-unavailable preflight, exact source/edge traceability, stale neutralization, source loss through the production batch boundary, no event replay from held baselines or prior runs, offline block/layout retention, changed contracts and revoked tokens. Native profile service tests in this scenario also show that copies exclude machine assignments, editing a copy leaves Default and the source graph intact, and previously captured applied documents are immutable. Opening, copying, exporting and saving do not activate a profile.

Run the four portable integration cases with:

```sh
cargo test --locked --manifest-path crates/profiles/Cargo.toml --test complete_rig
```

The existing per-module 71% coverage gate remains unchanged. Its `--lib` measurement does not include Cargo integration tests, so CI runs this scenario explicitly in addition to that gate.
