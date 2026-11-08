# Direct USB waits on the shared serial topology owner

During an Apply or Save recovery, a missing exact USB endpoint now waits for
the workspace's existing native device notification, shutdown, or its
original reconnect deadline. Recovery registers an operation-scoped weak
wake. The workspace commits serial inventory before signalling it. Raw
device-interface invalidations wake the worker even when the before and
after COM rows compare equal. The resumed command still checks the exact
port and confirms module identity, boot, generation and readback.

A port that is present can still need an open or firmware readback retry.
That retry keeps the existing 50 ms delay within the command budget.
If the native watcher or its current port inventory is unavailable,
recovery retains the same bounded retry. The shared owner wakes waiting
commands on coverage changes and clears coverage before shutting down its
native subscription. The write is issued once.

## Qualification scope

The missing-port fixture observed no second enumeration during a 140 ms
quiet window. A shared invalidation supplied the returning row. The worker
made one exact revalidation and one open. Other fixtures exercised loss of
coverage, a returned port whose first open failed, and direct shutdown while
waiting for the missing port. A fleet fixture exercised the command finishing
before the inventory reader had adopted the returning port. The fleet used
the retained topology commit to admit the worker's fresh connection.

Windows registration and coverage loss and restoration are verified with an
authored inventory. Real hardware re-enumeration remains in #199, including
configuration settle and firmware response timing. Linux and macOS runtime
qualification remains open.

The rebuilt Windows dev Cockpit passed its passive USB metadata and input
view fixtures. Those fixtures use a private database and exercise gateway
and observation handling with an unobserved endpoint.

The final Windows suite passed 2,447 tests with 18 default ignores. Both
pinned DCS archive tests passed separately. All eight changed production
modules exceeded the 71% native line coverage requirement. Direct USB
recovery reached 95.24%, the shared wake source 100%, and the workspace
worker 95.10%.

Dev executable SHA-256
`c8bf18570e2122aa29963800b42da091975316c21724fb1fc648bce83624ab05`.
