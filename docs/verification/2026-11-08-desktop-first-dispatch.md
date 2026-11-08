# Desktop first-dispatch rerun

The native fixture now marks the first press and release from each of two
authenticated WSS sources separately from the sustained phases. The analyzer
reports the median and first dispatch per source and checks native submission
order within each source. The UI load uses real Overview and Profiles
navigation in Cockpit without taking OS foreground. A separate UI-only smoke
run observed 36 view transitions in 4.51 seconds and returned to the visible
saved profile list. The 21 portable timing tests passed, with 90% line coverage
of the analyzer. The native workload is exercised through the opt-in fixture.

The timed run used an audit-enabled native build of `1a4a8ff8` with executable
SHA-256 `53241df72ed320c49431549560893ec5a16b43fb10dec5fe18a76d615c462649`.
Run `r4-desktop-ff10804474` used a disposable Cockpit database and a dedicated
receive-only window. It reached the first idle warmup. The exact receiver
observed an injected KeyA down and up while it had foreground. The worker
recorded a successful `SendInput` return about 112 ms after dequeue, past the
parent transport's 100 ms response deadline. The worker recorded a successful
release during cleanup. The parent retained KeyA as a possible hold because
it did not receive the Stop acknowledgement. Neither sustained phase ran.

Reopening the same isolated database with audit disabled showed
`recovery-needed` with no active owner or applied snapshot. The test receiver
and input worker had already exited. The fixture database and failure trace
were retained. No recovery input was sent after the exact receiver exited.

The incomplete parent trace and missing Stop acknowledgement preclude a
capture-to-submission percentile or a cleanup pass. This run leaves first
dispatch, bursts and mixed-load software qualification in #193 and #198.
Physical controller and simulator checks remain in #199. Local evidence is in
`.research/reactive/r4-desktop-timing/run-5645d72a/`.
