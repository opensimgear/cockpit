# Companion profile preparation

Native profile preparation reads the protected paired-installation record and retained immutable panel cache without creating missing state, TLS identity, listener, or USB transport. A desired panel may differ from the current assignment. The native-only proof pins its paired installation, current assignment, retained publication, capability, and Hub lifetime; it grants neither input admission nor delivery authority. Assignment edits (including ABA and uncertain persistence), revocation, publication pruning, and another Hub invalidate the proof.

Explicitly configured shared companion detection and listeners keep their existing behavior. The separate journaled acquisition entry point ensures shared transports are available; profile Stop must not claim exclusive ownership of or tear down shared connectivity.

Verification on Windows:

- Four focused portable proof/storage tests pass; the full companion service suite passes 77 tests.
- Nine native companion-host tests pass with the Windows user credential context. Retained metadata inspection leaves the protected bytes unchanged and creates no TLS files or server.
- `scripts/companion-coverage.py --output-dir .research/profile-companion-preparation-coverage` passes the unchanged 71% gate for all 13 modules. New `profile.rs` is 106/106 (100%); native `companion_host.rs` is 349/470 (74.26%).

This unit does not implement profile assignment ownership, live companion subscriptions, feedback leases, or the profile Host adapter. Those require the subsequent P5 ownership and delivery units. Fixture tests do not qualify physical companions, browser rendering, or game output.
