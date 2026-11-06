# Explicit launch startup notifications

Explicit profile launch now waits on the same notification during startup
and process confirmation. Committed process observations, catalogue changes,
hold changes and cancellation wake the request. The startup scan still
checks the exact executable and process identity after a wake and checks
admission again before using the result. The 45-second deadline remains.

The activation observer signals entry and exit from service. While the
observer is running, the launch request has no 25 ms cancellation recheck.
An unavailable observer retains the existing 100 ms fallback. The
observer's separate Windows discovery fallback remains in the acquisition
inventory.

The rebuilt Windows dev Cockpit launched a hidden, output-free fixture
from idle and applied the default profile after verifying the new process.
The fixture was stopped and its temporary catalogue entry removed. The
attached-controller routing and workspace checks also passed. Simulator
telemetry and Windows process-event permissions remain open.

The first full local run hit a DCS acquisition-clock fixture failure
before the fixture obtained a prepared context. The exact test binary
passed five focused reruns of that fixture.

A second full invocation passed the DCS fixture and failed the offline companion
reconnect fixture, which had passed in the first invocation. Running the two
fixtures separately yielded two passes, and the rest of the suite passed in a
single invocation with 2,359 passes and 18 explicit ignores. Across the three
successful invocations, 2,361 tests passed. Both original full invocations
retained one failure each.

Measured production line coverage from the successful invocations was 98.51%
for the activation observer, 96.85% for its runtime, 95.82% for game launch,
and 90.95% for the launch orchestration runtime. The 247-entry timing audit
passed. CI remains deferred until feature implementation is complete.
