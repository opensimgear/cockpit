# Native companion profile session composition

The unified native Host now prepares, journals, acquires and releases companion panel assignments through the existing retained Hub and native `CompanionProvider`. The portable profile remains the authoring source. No browser values, legacy feedback route store, fabricated delivery token or additional transport owner is used.

Preparation reads the selected paired instance and desired retained publication. It can prepare publication B while the actual panel still has A; it does not change observed inventory or the panel. Only exact assigned port variants consume the bounded prospective catalogue. Empty Default preparation does not initialize companion storage, TLS, listeners or USB services.

Before takeover, the journal captures compact protected assignment receipts with a fresh UUID per prepared run. Overlapping selections inherit the original baseline from the exact outgoing stamp. Omitted instances receive release-only receipts, so applying Default restores the original panel. Acquisition follows the durable phase intent; it preserves the shared listener and detection setup. Stop, compensation and restart recovery only compare and restore still-owned assignments. They never replay a profile panel or rearm its input.

Baseline waits for the real panel's neutral session and cached native observation of the exact desired publication and capability fingerprint. Read tokens are bound independently of feedback permission. Start recaptures the actual baseline, compiles current delivery tokens, and arms feedback only after all selected writes are available. No discovery runs inside Baseline or health.

During health polling, native source loss and stale Read/Write tokens withdraw that companion before reducer timers advance. Each instance can obtain its own fresh delivery generation and baseline after reconnect; an offline peer does not prevent another instance from recovering. Rebaseline discards buffered transitions, and unchanged tokens do not reset running timers. Feedback is published after the reducer advances and remains truthful local presentation; it does not claim HID submission or game confirmation. The legacy global feedback clear path cannot clear an owned panel.

Verification on main:

- Nine companion adapter tests cover exact contracts, source ordering, feedback status, fresh tokens, output-only profile branches, buffered-edge suppression, peer-isolated reconnect, and rejection of an implicit Network/USB transport transfer even when the route ID is unchanged.
- Six Host/helper tests cover prospective publication mismatch without mutation, compact receipt takeover and omission, operator-preserving recovery, actual wire/native-provider activation, distinct durable owners across one coordinator session, Default restoration, crash recovery without replay, and stale preparation preserving the prior run.
- The complete native `profile_session_` group passed before the final stale-preparation case was added; that case and the other five Host cases then passed together. Four copied-executable Manager profile tests also passed.

The actual Host fixtures use the existing temporary TLS/DPAPI setup under the normal Windows user. Sandboxed DPAPI access fails as expected; the same fixtures pass with normal-user access. Qualification gates for virtual HID, hiding and live script output remain in place. A profile may use only companion feedback, but the currently supported publication contract still requires 1–128 interactive controls; a zero-control publication is explicitly unsupported.

Coverage is measured by the unchanged 71% per-module native runner, including the companion Host helper and the existing guarded I/O production code. Test-only fixture constructors are placed after production so the existing coverage parser counts the full production denominator.

The accepted source-stamped run is `.research/profile-companion-host-coverage`: companion Host helper 194/242 production lines (80.17%), companion adapter 740/823 (89.91%), guarded I/O 186/191 (97.38%). It runs the existing native filter set plus the six actual Host/helper lifecycle tests. The stamped sources were unchanged from the instrumented build through the final measurements.
