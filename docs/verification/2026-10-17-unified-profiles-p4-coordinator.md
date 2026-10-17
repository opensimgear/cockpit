# P4 profile coordinator core — 17 October 2026

This unit adds the native `ProfileSession` state machine and a bounded recovery manifest. It does not establish real hardware, driver or game qualification, and does not complete milestone #84.

The coordinator keeps the authoring document in memory as an immutable candidate. Preparation, validation and capture precede takeover; failures before the first durable intent leave the previous run effective. Each subsequent activation phase has a durable intent before effects. The previous owner is revoked before neutralization, and the replacement receives authority only after the baseline phase. Uncertain replies trigger compensation and a concrete recovery-needed state rather than replaying the mutation.

The manifest contains ownership identities, hashes, attempted phases and bounded native restoration receipts. It contains no executable graph, script source or replayable action. Startup only inspects it: an interrupted run requires explicit recovery. A process lock rejects overlapping manifest owners, and external edits, unknown or missing envelope fields, invalid phase sequences and oversized receipts preserve the evidence instead of silently replacing it. Adapter restoration must independently validate its exact receipt and compare current ownership, boot and revisions.

Pause persists a latch, stops owned work and releases hiding. A failed latch write still revokes and neutralizes the in-memory run, leaves recovery required and reports the storage error. Automation and metadata selection cannot clear this latch. Resume alone never reapplies a graph; a fresh Apply is required. Empty Default can be applied without optional backends. Automatic startup and profile selection remain P8 work.

## Automated evidence

The existing setup coverage runner now includes `profile_session.rs` and `profile_session_journal.rs`, with the unchanged 71% per-module gate. The first complete instrumented native run passed 508 tests, with three existing ignored tests, followed by the separately invoked official pinned DCS-BIOS archive checksum/smoke test. All six retained setup/context modules also passed (83.22–98.46%). This baseline is under `.research/profile-session-coverage`.

After the Pause durability correction, all 13 focused coordinator tests and the official pinned smoke test passed against the rebuilt instrumented binary. Final coverage is **92.71% (305/329 lines)** for the coordinator and **92.23% (190/206 lines)** for the journal. Evidence, annotated lines and exact source hashes are under `.research/profile-session-core-final`. The focused invocation uses `--module profile_session.rs --module profile_session_journal.rs --test-filter profile_session::tests`; CI retains the default complete native suite and all eight production modules at 71%.

The focused fault-injection cases cover every activation phase, failed preparation/capture/first journal write, prior-run preservation, stale owner rejection, switched baseline ownership, source degradation, game exit, persistent Pause, restart recovery without replay or rehiding, offline/uncertain restoration, overlapping owners, external edits and malicious journal envelopes. The final regression verifies failed-durability Pause and idempotent Resume on an already running owner. The normal native binary also passes these 13 tests and all 19 Manager tests, including three new session API cases; Manager wiring remains a separate unit.

## Remaining integration scope

The production `NativeProfileHost` adapters must connect these phases to the existing mapping reducer and typed worker, verified game-context broker, virtual-controller and HidHide journals, companion ownership and physical route-pinned runtime overrides. Preparation capabilities must remain distinct from fresh delivery tokens; no baseline may be fabricated to bypass a compiler readiness block. Required missing resources, unsupported or unqualified backends, layout/visibility errors and route conflicts must block takeover.

Manager Apply/Stop/Pause/Recover IPC, native health polling and guards for independent mutation paths are a separate unit in progress. Saving or navigating profiles must remain independent of activation. Actual adapter fault tests, fresh native baseline integration, compensation under changed physical state and the full #84 acceptance checklist remain open until those units are tested. Existing hardware qualification issues remain open.
