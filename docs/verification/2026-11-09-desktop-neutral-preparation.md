# Prepare desktop output at neutral

Cockpit queues an empty native owner after it verifies the current neutral
level and exact producer token. Focus return still requires a fresh neutral
observation for the new admission. The native gate checks the token, scope
and queue epoch again before Arm. Loss clears pending preparation. Actual
delivery keeps its existing durable intent and acknowledgement commits.

The existing output worker prepares one route per idle ticket. Authored
deliveries, Pause, Stop and lease renewal take precedence. Profiles with
larger owner sets or scope proofs arm on first use to preserve recovery
journal capacity.

All 181 Windows mapping-host tests and 22 module coverage gates passed.
Production line coverage was 93.41% in desktop dispatch, 94.69% in desktop
output, 98.72% in the output worker and 100% in the executor. The tests cover
startup neutral preparation, focus return, a blocked Arm with concurrent
source revocation, outstanding lease authorization and the journal capacity
guard. All 22 portable desktop timing tests passed.

Native run `r4-desktop-93cd0325d5` used an audit-enabled dev build of
`a549a1a0` plus this change, with executable SHA-256
`6cd86e880e2b8c6a95b7c2d688d33991b6447d58462b496a7e930e898d93295f`.
The scoped receiver and two authenticated WSS peers established four empty
native owners before the first press. The warmup had no Arm requests, lost
edges or audit errors. In the four-edge sample, WSS admission to successful
`SendInput` return measured p50 8.155 ms and p95/p99 18.693 ms. The two first
presses took 8.155 and 18.693 ms. The prior build measured 14.772 and 32.203 ms
for its two first presses. The strict 5/10 ms budgets remain open.

Each source sent 98 key transitions in sustained idle. The receiver logged
all of them plus the two warmup edges per source. There were 37 native
pointer-batch requests, but no receiver `WM_MOUSEMOVE` observations. The
mixed workload stopped before the UI-load phase. Stop and wrapper cleanup
confirmed no held controls or retained worker. Local evidence is in
`.research/reactive/r4-desktop-timing/run-0161c950/`.

The measured path is WSS to Windows keyboard submission. Physical controller
preview and raw-input-to-virtual measurements remain in #199.
