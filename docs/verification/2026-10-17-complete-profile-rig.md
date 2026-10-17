# Complete portable profile rig

This is fixture evidence from the real resource registry, profile compiler and typed mapping reducer. It does not verify vJoy, HidHide, a connected device, a running game or a companion display. The captured backend uses `BackendCapabilities::default()`, so native activation and hiding remain blocked by qualification.

The repository scenario is `tests/fixtures/profiles/complete-rig.json`. Its replay and independently specified expected samples/requests are in `complete-rig-replay.json`. The replay pins the document SHA-256 with checkout line endings normalized to LF. The integration test is `crates/profiles/tests/complete_rig.rs`.

The graph contains:

- Third-party A and B feeding one named virtual controller, plus one actual companion control.
- Multiple OSG modules feeding a second named virtual controller.
- Third-party C with a hide-while-active intent and no invented signal route.
- A separate OSG event source feeding an explicit atomic DCS command invocation.
- The CommonData `ALT_MSL_FT` raw integer feeding **Interpret MSL altitude**, which explicitly declares **Reported MSL altitude**, in `ft`. Visible conversion, hysteresis, conditional display and formatting follow it. The resulting state feeds a companion warning widget and a text widget; a separate visible setter requests a physical indicator change.

The raw source contract is integer 0–65535 with no unit, 1500 ms freshness and
unknown fallback. The declaration preserves its exact type, bounds and value;
the following unit conversion produces metres in 0–19975.068. The registry's
observed port is `channel:common:CommonData:ALT_MSL_FT:0`, captured with its real
registry contract hash in the local assignment. The portable resource keeps its
logical `altitude` port. This fixture models the catalogue contract and the
author's explicit quantity interpretation; its input frames remain injected
recordings, not a running DCS game. The graph has nine resources, seven
processing nodes, two virtual controllers and 18 exact bindings.

Both CAN-only and direct USB variants run the same portable graph. The real registry correlates the USB and CAN observations into one resource, keeps both connection attributes and selects the higher-priority USB input while both routes advertise usable baselines. Gateway, serial endpoint and route metadata never become portable graph edges or exports.

The separate command module's injected publisher observation is configured
disabled. Other OSG observations remain published; the fixture does not claim
that the editor's publication intent has changed hardware. The actual compiler
accepts the command branch under the disabled observation, and an explicit
republished observation blocks it with `dcsHidPublicationRequired`. Native
publication admission and restoration are separate evidence.

The replay checks exact computed controller states, raw `10000`, declared
`10000 ft`, converted `3048 m`, conditional warning state and formatted
`ALT 3048.0 m`. It compares complete requested feedback and physical setter
tuples. Advancing beyond the real 1500 ms freshness marks the source and declared
output stale. DCS requests are zero-argument atomic one-shot tuples; none is
interpreted as a device or game acknowledgement.

Additional assertions cover native-unavailable preflight, exact source/edge traceability, stale neutralization, source loss through the production batch boundary, no event replay from held baselines or prior runs, offline block/layout retention, changed contracts and revoked tokens. Native profile service tests in this scenario also show that copies exclude machine assignments, editing a copy leaves Default and the source graph intact, and previously captured applied documents are immutable. Opening, copying, exporting and saving do not activate a profile.

Run the four portable integration cases with:

```sh
cargo test --locked --manifest-path crates/profiles/Cargo.toml --test complete_rig
```

The existing per-module 71% coverage gate remains unchanged. Its `--lib` measurement does not include Cargo integration tests, so CI runs this scenario explicitly in addition to that gate.

Raw-quantity update result on 2026-10-17: **four replay/graph cases and five
editor-metadata cases passed**, including storage/export independence. The
corresponding [Edge authoring test](2026-10-17-complete-profile-authoring-ui.md)
also passed with every IPC intercepted; it is separate frontend evidence.
