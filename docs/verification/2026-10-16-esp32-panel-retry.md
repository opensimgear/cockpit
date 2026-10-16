# ESP32 unsupported-panel reconnect loop — 2026-10-16

Observed live Sunton diagnostics: USB ready on COM6 with saved serial setup; device reported panel-unsupported-corrupt-or-over-budget after its open request. The assigned Panel 1 now includes an XY pad. ESP32 publication decoding supports button, indicator, text, numeric and gauge widgets, so this content is rejected. It was not a missing-publication startup failure or physical USB disconnection.

The USB worker previously retried every error state every three seconds. It now remembers the assigned publication hash when a permanent unsupported/corrupt/over-budget report arrives, and suppresses automatic reopening of that same hash. Removing/changing the assignment permits recovery. Normal transient connection retries are unchanged.

Added regression test covers initial retries, blocked same-content retry and release on assignment change/removal. USB suite: 13 passed. Frontend checks pass. The Windows mirror and running native app include the fix. A temporary USB diagnostic view used to confirm the cause was removed; the detail view uses the existing floating notification system to explain the panel incompatibility.

The saved panel has not been edited. A choice between adding XY-pad firmware support and assigning a compatible copy was requested; that follow-up remains pending.
