# Selected Windows rig: native qualification preparation

The selected real acceptance rig is DCS MH-6J, both VKB joysticks combined, the T-Rudder hidden, and a browser companion. The user explicitly deferred actual OSG hardware validation because none is available. This preparation does not mark the combined acceptance complete.

`scripts/prepare-native-profile-qualification.py` prepares a new private copy from a full reviewed Git commit and an explicitly reviewed hash of `tests/manual/native-profile-qualification.rs`. Its default is a plan. It never builds or runs the result. The ordinary desktop entry point and qualification flags are unchanged.

The private copy substitutes a manual entry point with no desktop or `GatewayManager` fallback. It composes the real native profile Host/Coordinator, controller registry, game observer/provider and browser companion service, with empty OSG endpoints. Its three explicit private qualification overrides allow controlled testing of the installed vJoy/HidHide backends. All other existing-file changes are confined to entry/module wiring and exposing the existing empty panel helper. These overrides are not release qualification.

Before hiding, an independent Windows process must acknowledge the exact native preparation, complete original HidHide policy, selected pedal interfaces and sealed visibility receipt. It retains a handle to the actual parent process and uses the existing compare-and-restore journal path after proven parent termination. Foreign policy changes are preserved as conflicts. Setup mutations retain the normal Coordinator admission guard. Read-only inventory and explicit Apply are separate operations.

## Recorded preparation and read-only run

The reviewed production baseline is `ebdc742f4dc77b3e44b215ad020714eba3792001`. The manual source SHA-256 is `649e468d013a04d236bf24d7b20765550d78b6bc45cc72c57e19c446ee3bd48e`; preparer SHA-256 is `44cd8ad3efe0444727b662832adc439b9f58863aa3926fc5c7f5706e5b370af0`.

Independent source review, Rust formatting/parsing and all nine mandatory pure preparer checks pass. Windows symlink creation was unavailable, so that OS exercise is explicitly untested; the existing lstat/reparse rejection was reviewed. The new copy preserves 518 reviewed files unchanged before building. Existing dependency reuse was checked against the exact lockfile and all tracked package sources. The browser build passes with 129 modules. The native build passes in 55.08 seconds. Tauri regenerates nine permission TOML files with different line endings; their complete normalized bytes match the reviewed originals, and all remaining source hashes remain exact.

The executable identified by Cargo JSON was copied before use. Its SHA-256 is `0e4adbc5dca08934b37ebf21c1d4aac4c5c6b741ad5c04af21df87ce3facc43b`. A real read-only preflight succeeds. Both VKB Gladiator EVO controllers and the T-Rudder are online with current native baselines. The installed vJoy slot 3 is free and exposes X/Y/Z, 16 buttons and two continuous hats. The original HidHide policy is active with three unrelated hidden entries and four unrelated application exceptions; none of the selected VKB/pedal interfaces is already hidden.

A separate read-only Serve smoke passes Ready, Status, Preflight and explicit Shutdown. The independent guardian confirms the exact parent identity and termination, reports no armed restoration, and exits. The complete observed native HidHide policy matches the original read-only snapshot. No Apply, controller claim, output submission, hiding mutation, assignment, game launch or OSG discovery occurred.

Private evidence is retained under `.research/native-profile-qualification-design/` and `.research/native-profile-qualification/ebdc742-build-1/private-state/`, including the source/build manifests, actual Cargo artifact record, generated-file comparison, complete native preflight, Serve replies and guardian report. Device paths and machine-local identities are kept out of portable examples.

## Remaining acceptance

The selected profile still needs exact native port assignments, actual browser pairing/publication, guarded Apply, real control movement, game-confirmed output and visibility, lifecycle/loss checks and verified restoration. The current DCS-bound VKB controls are right X/Y, left Y and both POV1 hats; that scope is not a claim that every physical control is mapped. Pedals are hidden-only and are not silently routed into the combined output.

The manual wrapper does not establish the normal desktop's saved-profile workflow, automatic rule activation or native game-launch orchestration. Its extra guardian also does not install parent-death restoration into the ordinary app. Passing this read-only run does not enable any original qualification flag, prove active crash restoration, or satisfy the combined real-device/game acceptance.
