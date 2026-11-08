# CAN configuration activation from publication events

The STM32 peripheral replies to Apply before finishing the configuration
change. Finishing activation advances its configuration generation and
announces the new generation over CAN. The gateway publishes a sequenced
change for the replacement assignment and another when the first accepted
heartbeat or input makes the route usable.

Cockpit records the publication cursor before sending the write. While the
write is pending, it waits for a newer record, stream loss, shutdown or the
existing recovery deadline. A snapshot must show the original peripheral
boot and the next configuration generation before active readback. A
successful read must confirm that generation in both the settings payload
and the gateway inventory. The actor discards a mismatched reply before
publishing the command result. A route rejection waits for further evidence
and retries only the read. Snapshot, reconnect and readback share the same
confirmation deadline. The bounded retry remains available when publication
coverage is absent.

This uses the existing gateway firmware contract at
`dfd83084bf8a48a40d4db15b59839978b7674b9b` and the existing STM32 module
firmware at `c5457884793ff2cf91291ee0077d9411bc98d496`. The Pico Debug
firmware completes its virtual device Apply synchronously.

## Local verification

The serial fixtures exercise a quiet feed, an unrelated publication, a
generation change received before the pending reply, a delayed replacement
route, a peripheral reboot, a stale readback and a stalled readback. Each
fixture sends the configuration write once.

The seven focused native tests passed. The rebuilt dev Cockpit passed the
workspace baseline, USB metadata and USB preview checks using a private
database. Executable SHA-256
`742d61d956bf1f23205159e2b9571cffa0effcdc347b17a93f4b173331c1724d`.
The 27 workspace frontend tests passed with 99.22% lines, 89.51% branches
and 89.19% functions covered. Both pinned DCS archive checks passed.
The same executable passed the native profile editor fixture. A clean editor
refreshed once after an external rename and a dirty draft stayed intact. The
first harness run tried to import its test modules before the WebView mounted.
Waiting for navigation readiness resolved the fixture startup error.

The full native run passed 2,461 tests with 18 default ignores. The
companion reconnect case that failed during the previous milestone passed
in this run.

| Module | Production lines | LLVM functions |
| --- | ---: | ---: |
| STM32 gateway | 89.30% | 91.73% |
| Recovery waits | 96.00% | 94.74% |

Physical CAN configuration activation remains in #199.
