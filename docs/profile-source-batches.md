# Native source batch admission

The native profile host drains HID, game and panel captures through one bounded `Phase::accept_admissions` call before advancing the mapping clock. Direct controller/panel frames and native simulator evidence share the same loss-before-deadline transaction. The reducer accepts at most 4,096 frames and 32 MiB per mixed drain. Native preflight rejects malformed or oversized transactions before installing authority or advancing its clock; the host stops on admission failure.

Providers are independent causal streams. Their drain order cannot establish that one provider was still available when another provider captured a later frame. The reducer therefore validates the whole batch and revokes all observed source losses, replacement baselines and non-known port samples before any frame can run timers. A sequence discontinuity, generation change or explicit gap requires a newer complete baseline. A disconnect keeps its distinct unavailable status. A malformed batch fails without advancing its clock.

Loss cancels dependent script tickets and timers, removes pending sink effects and suppresses same-source input occurrences captured before the loss. Only later complete baselines restore a lost source; their event levels never count as occurrences. Later genuine changes may produce actions. Port-specific unknown, stale and unavailable samples revoke dependent work while preserving independent ports and branches.

Source loss and recovery reevaluate only the affected source's descendants, alongside any genuinely due work. Another source's baseline cannot enqueue callbacks on an unchanged script branch. Actual baselines explicitly seed synchronous built-in descendants: held buttons cannot start edge-triggered actions, while a neutral baseline arms the next genuine press. Ordinary updates and script transactions that suppress event outputs do not perform this reset.

Virtual reports contain a whole controller. Revocation clamps only affected control slots to the recomputed safe value, preserving queued press/release transitions on independent controls. Independent actions and timers continue after all revocations have been installed.

The single-frame `Phase::accept` API remains available for an already ordered Direct causal stream. Callers draining independent Direct providers must use `accept_batch`, and mixed/native providers must use `accept_admissions`; sorting loss frames first does not suffice because the first admitted loss could otherwise advance timers before later losses are revoked.

The mapping library regressions cover mixed HID/panel/game timers, already queued actions, sequence and generation recovery, non-known samples, affected virtual button/axis/hat slots, independent pulse retention, malformed frames and batch overflow.

## Native acquisition authority

Before the first native baseline, the host pins the controller journal clock and
the complete source membership, family and sampled/latched policy of every port.
Missing fields stay registered. Each native envelope carries deduplicated tables
of original capture positions and validity leases; contributors refer to exact
native bytes or channel identities. Future, contradictory, regressing or foreign
clock evidence is rejected. Capture conversion uses the original acquisition
Instant, never the time a reader or UI receives it.

A family baseline closes every prepared member and advances its continuity.
BIOS and ownship can withdraw and recover independently. A logical source
generation replacement closes every family. Native loss revokes only dependent
work; a healthy independent family's genuine update remains an update even in
the same envelope as another family's baseline.

Sampled deadlines use the earlier of provider validity and original capture plus
port freshness. Latched BIOS bytes retain their original acquisition while a
genuine counter observation renews a separate validity lease. Unchanged valid
lease renewal does not evaluate the graph, enqueue scripts or rearm rules.
Recovery seeds current levels without replaying held presses. Quiet Direct
liveness cannot renew native evidence.

Timers, queued script requests, in-flight callbacks and output effects retain
immutable original contributor bindings. Newer source samples cannot lend their
freshness to old work. A still-valid old request may complete; expired or rebased
work retires, including an in-flight FIFO head, and a late reply cannot affect its
successor. Desktop rearming additionally requires current native level evidence;
an old causal pulse completion cannot manufacture a new neutral observation.
Authored fallback quality may remain Known without granting native rearm evidence.

Bounds include 65,536 capture entries, 64 leases, 1,048,576 contributor references,
16 MiB per envelope and 32 MiB retained native evidence (including owned map keys,
samples and shared proof allocations). Native validity intervals are capped at
11 seconds. The Direct-only path avoids cloning the native ledger.

## Producer commit notifications

Canonical BIOS, shared ownship and X-Plane journals notify the profile worker
directly on committed reports, generation replacement, expiry and closure.
`NativeWatch` is a passive notification lease, separate from a data subscription:
it owns no payload queue, never grants freshness or commands, and cannot retain
the producer socket. Watches and data readers share the journal's bounded reader
capacity. Registration atomically requests an initial drain, covering data that
arrived before the watch was installed. Stop and reader replacement drop leases.

The shared `NativeWake` signal advances an atomic epoch and unparks the bound
worker. It runs no consumer callback, graph, codec or IPC work. The worker observes
the epoch before draining and checks it before parking, so a notification remains
visible if another wait consumed the unpark token. Binding preserves notifications
that predate the worker handle; epoch exhaustion disables parking instead of
silently wrapping. Coalescing wake signals does not coalesce ordered input data.

Remaining context, inventory and adapter service timers are explicit fallback
work; this notification path does not claim the entire profile is tickless.
Notification admission failure is diagnosed while existing service remains active.
It neither extends nor withdraws otherwise valid source evidence. Every actual
read and command still performs the existing owner/continuity/validity checks.
