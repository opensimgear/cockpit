# Native source batch admission

The native profile host drains HID, game and panel captures as one bounded `Phase::accept_batch` call before advancing the mapping clock. The reducer accepts at most 4,096 frames per drain. Oversized batches fault and neutralize the phase; they do not retain previously queued actions.

Providers are independent causal streams. Their drain order cannot establish that one provider was still available when another provider captured a later frame. The reducer therefore validates the whole batch and revokes all observed source losses, replacement baselines and non-known port samples before any frame can run timers. A sequence discontinuity, generation change or explicit gap requires a newer complete baseline. A disconnect keeps its distinct unavailable status. A malformed batch fails without advancing its clock.

Loss cancels dependent script tickets and timers, removes pending sink effects and suppresses same-source input occurrences captured before the loss. Only later complete baselines restore a lost source; their event levels never count as occurrences. Later genuine changes may produce actions. Port-specific unknown, stale and unavailable samples revoke dependent work while preserving independent ports and branches.

Virtual reports contain a whole controller. Revocation clamps only affected control slots to the recomputed safe value, preserving queued press/release transitions on independent controls. Independent actions and timers continue after all revocations have been installed.

The single-frame `Phase::accept` API remains available for an already ordered causal stream. Callers draining independent providers must use `accept_batch`; sorting loss frames first does not suffice because the first admitted loss could otherwise advance timers before later losses are revoked.

The mapping library regressions cover mixed HID/panel/game timers, already queued actions, sequence and generation recovery, non-known samples, affected virtual button/axis/hat slots, independent pulse retention, malformed frames and batch overflow.
