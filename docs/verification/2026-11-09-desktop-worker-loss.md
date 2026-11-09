# Abrupt Windows desktop worker loss

A running Cockpit at `bc7197156874b155d386f9e1c2660b37c8f9859a` applied a
keyboard-only profile against a private Windows receiver. The test enabled
`OSG_DESKTOP_LATENCY_NATIVE=1`, `OSG_DESKTOP_NATIVE_INJECT=1` and
`OSG_DESKTOP_WORKER_LOSS_NATIVE=1` with a disposable SQLite database.
The fixture pinned the private worker process handle after checking the
foreground receiver, process ancestry and durable KeyA ownership.

| Step | Observed result |
| --- | --- |
| Terminate the worker with KeyA held | The original Cockpit process stayed alive and entered Recovery needed. The SQLite journal retained KeyA as held and possibly held, with cleanup incomplete and ownership uncertain. |
| Attempt Apply before recovery | Cockpit rejected Apply. The receiver still showed only the original KeyA down. |
| Recover | The receiver got exactly one KeyA up. The same journal cleared its held, possible and in-flight state and marked cleanup complete. |
| Re-Apply | A fresh worker started in the next session generation. The held value caused no replay. A fresh neutral sample allowed the next KeyA down and up. |
| Stop | The receiver recorded exactly down, up, down, up. Cockpit confirmed clean ownership, and the wrapper restored settings and closed the receiver without errors. |

The portable desktop suite passed 34 tests. It and the instrumented native
run covered 215 of 230 executable lines (93.48%) in the Python fault fixture.
The ordinary native keyboard-only timing fixture also passed its delivery
and cleanup assertions with worker-loss mode disabled.

Local evidence is in
`.research/reactive/r4-desktop-timing/worker-loss-773b324d55/`.
The Cockpit executable SHA-256 is
`b1eb29dd1c65e128d4084d9bb9584e74b042c33b50c7727dea99f93ba4635966`.
The `evidence.json` SHA-256 is
`94b4b912ee79b657b23f1d65a6532268e40935023b8a6d157f6712fd7bfcf8ed`.
The parent audit checkpoint was complete and valid through sequence 238.

This qualifies the held-key recovery case in #193 and #198. The strict
desktop timing and mixed pointer gates remain open. Physical controller
preview and output qualification are tracked separately in #199.
