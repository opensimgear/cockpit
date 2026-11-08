# Regression guards for #188

| Requirement | Executable guard |
| --- | --- |
| Periodic state queries | `scripts/reactive/ui-timers.test.mjs` rejects unclassified intervals and frames, altered allowlisted calls and stale entries. The live `tests/ui/reactive-idle-native.mjs` gate observes gateway traffic on each top-level view. Both clients previously recorded [zero gateway requests across eight idle views](2026-11-08-idle-transport-matrix.md). |
| Snapshot races | `crates/streams/src/registry/tests.rs` races a subscriber against commits in `concurrent_snapshot_boundary_never_skips_a_committed_change`. It also checks that latest-state delivery preserves real continuity boundaries. |
| Subscription leaks | `cancellation_and_client_close_wake_and_release_caps` checks that cancelling readers reclaims registry capacity. `owner_retirement_closes_live_readers_and_fences_late_old_owner` checks retirement and rejection of old-owner data. Both run in the stream suite. |
| Reordered edges | `ordered_repeated_transitions_use_positive_native_sequences_and_host_clock` checks companion edge order. The stream suite checks ordered deltas and exact counters in its shared wire fixtures. |
| Safety starvation | `bounded_per_instance_fair_drain_does_not_allow_a_hot_peer_to_starve_others` checks fairness. `blocked_native_channel_does_not_block_snapshot_renew_or_reserved_pause` and the blocked desktop Arm fixture verify that routing and revocation can progress while a destination is stalled. Both run in the mapping-host suite. |

The timing ledger accounts for acquisition and justified lease, expiry,
retry and presentation clocks. The committed audit checks that each site
has a recorded owner and disposition. The source audit and the live idle
fixture are separate guards. See the [acquisition ledger](../reactive/acquisition.md).

On `17407308`, all 60 stream tests, five frontend timer guard tests and the
three committed timing audits passed. The audits checked 247 pinned and 465
current sites. The 181 mapping-host tests passed on `08142f5b`, which has the
same mapping-host sources. See [desktop neutral preparation](2026-11-09-desktop-neutral-preparation.md).

These guards satisfy the parent regression-guard checklist item. The R0
representative baseline, the remaining integrated performance and platform
checks and the final CI run retain their own gates.
