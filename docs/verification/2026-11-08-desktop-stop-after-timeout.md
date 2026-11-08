# Desktop Stop after a worker reply timeout

The parent can now send terminal Stop after an exact queued request times out.
It discards only that request's late reply and requires the subsequent Stop
acknowledgement before clearing uncertainty. Both requests keep their 100 ms
response deadlines. Cockpit sends no new input work to the timed-out worker.
Malformed delivery acknowledgements still retain the explicit recovery fence.

The 195 Windows desktop-output tests passed, including stale or missing Stop
replies and an unacknowledged guarded key-down whose release ran before Stop.
All 29 desktop module coverage gates passed. Coordinator and transaction
production line coverage were 89.61% and 90.22%, respectively. LLVM also
recorded the timeout, exact late-reply discard, wrong-sequence rejection and
matching Stop return in the transport regression.

The final runtime candidate, SHA-256
`9340571be456d12221d8ed8fac18d8de35566e197a72f2c9e885d30e25b83564`,
completed both authenticated WSS warmup presses and releases with no lost
edges. Run `r4-desktop-96174fc381` produced a valid stopped audit. For the
four warmup edges, capture-to-`SendInput` return measured p50 9.548 ms and
p95/p99 32.203 ms. The two first presses measured 14.772 ms and 32.203 ms.
These samples miss the strict 5/10 ms budgets. The trace places most of the
first-dispatch time before worker dequeue, including serialized Arm and
delivery journal commits. The two first presses took about 0.64 ms and
0.50 ms from worker dequeue to successful native return.

The sustained idle run sent 393 frames and 99 key edges per source. The
receiver logged 102 edges per source, including two warmup edges and one
cleanup release each. The trace recorded 203 native pointer-batch requests,
but the receiver recorded no `WM_MOUSEMOVE` observations. The pointer check
timed out before the UI-load phase. Stop was acknowledged, the worker exited
normally, and both fixture cleanup checks found no held keys, uncertainty,
active owner or retained worker. The separate keyboard-only focus fixture
could not acquire foreground before input. Local timing evidence is in
`.research/reactive/r4-desktop-timing/run-32647aee/`.

These results leave desktop first-dispatch, burst and mixed-load timing open
under #193 and #198. The measurements begin at native WSS admission. Physical
controller preview and raw-to-virtual latency remain in #199.
