# Profile physical action worker

`PanelActions` composes the exact native endpoint lease primitive on bounded workers. It does not open a transport, use browser-owned route tokens, create a second journal, or change platform qualification. Preparation checks the immutable compiled physical sinks and produces the existing sealed per-endpoint receipts without claiming output. Required unsupported outputs block preparation; optional unsupported outputs are reported by `optional_unavailable()`. Dormant branches do not acquire devices. Duplicate native endpoint aliases and more than 16 endpoints are rejected.

The native Host integration sequence is:

1. `prepare(plan, owner, resources, panel_io)`, then add `capture_receipts()` to the coordinator manifest before acquisition.
2. `acquire()` starts bounded workers without waiting for serial I/O. `poll().acquired` reports completion. Each endpoint claims once through `claim_current`.
3. At Baseline, `bind_delivery(fresh_plan)` queues the exact fresh native Invoke tokens; it returns whether all required endpoints are bound. Binding is checked on the owner worker, followed by a guarded renewal. `start()` permits effects only once those bindings have succeeded.
4. `dispatch(&effects)` receives the whole drained typed-effect batch. It pre-revokes every endpoint with a known loss before admitting earlier tuples. `poll()` reads cached status and diagnostics; neither method performs transport I/O.
5. `stop()` immediately stops acceptance, clears queues and wakes workers. It returns pending until existing bounded operations finish and token-fenced release completes. The coordinator must retain receipts while `cleanup_complete` is false. A failed cleanup is reported, not silently marked restored.

A process-wide 16-worker budget covers retiring runs too. Each endpoint has a 64-entry FIFO; known action tuples are checked for exact names, order, scalar bounds and at most four arguments before copying into that queue. One-shot events are never coalesced. Every effect must match the native run, exact compiled sink, increasing sequence and monotonic effect time. Baseline and fallback cannot replay one-shot actions. The worker renews every second under the advertised five-second lease; no UI poll is required. Missing delivery binding expires the preparation after three seconds. Lease or delivery failure revokes that endpoint and discards its queued work. It is not automatically reclaimed when inputs reconnect.

An accepted request already in flight may finish after Stop or source loss. The same worker then releases that lease through firmware's compare-if-unchanged restoration; no later queued request may run. Independent endpoint workers continue while an affected endpoint is stopped. Overflows and invalid tuples revoke their endpoint. Required failures are surfaced through `ActionPoll.required_failed`; optional failures remain diagnostics.

`recover(owned_run, resources, panel_io)` creates cleanup-only actors from existing sealed receipts. Calling `stop()` starts only inspection and guarded release. Recovery cannot bind delivery, claim, Start or invoke. It never reconstructs a command tuple from a graph. Changed or unavailable exact native routes remain unresolved for explicit recovery, as described by the lease primitive.

The worker is tested independently of device framing: FIFO/no coalescing, delayed claim cancellation, pre-loss queue suppression, independent endpoints, overflow, stale run/sequence/sink rejection, baseline one-shot suppression, no-output initial unknowns, autonomous renewal failure, optional admission, global worker limits and unresolved cleanup. The lower-level USB/CAN lease tests separately exercise actual route/catalogue/argument and sealed-recovery contracts.

All 12 worker tests pass. The independent source-stamped native gate covers 378/488 production lines (77.46%) with the existing 71% minimum unchanged. Host composition remains a separate unit.
