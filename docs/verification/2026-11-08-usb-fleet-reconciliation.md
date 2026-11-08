# Direct USB configuration through temporary port loss

Apply and Save can make a USB interface disappear before the worker confirms
the configuration write. The fleet previously retired the worker on the
first missing-port inventory. That cancelled the worker's readback even
though the same device could still return within its recovery deadline.

The fleet now retains the admitted worker through that readback and publishes
an offline route immediately on confirmed removal. A returned COM port alone
leaves the route unavailable. Readmission requires a fresh connection
generation from the worker's identity-fenced readback. If the endpoint is
still absent or has moved when readback finishes, the old slot retires its
worker and cannot expose a route. The operator response distinguishes a
confirmed write from an unconfirmed result so a fresh connection need not
repeat the write.

Two fleet fixtures remove the registered port while an Apply-and-Save request
is in flight. One returns it on the same path and verifies a new route on the
original worker. The other leaves it absent and verifies retirement and no
route. Both count one mutation. The normal direct USB worker tests also
exercise transport readback and the no-replay guards.

The native Windows suite passed 2,439 tests with 18 default ignores. The two
pinned DCS-BIOS archive tests passed separately. USB fleet production line
coverage was 91.65%, above the configured 71% requirement. LLVM reported
66.88% function coverage for the file. The configured native USB gate uses
production lines.

The rebuilt Windows dev Cockpit passed its USB metadata and input view checks
in an isolated session. These checks verify passive inspection and rejection
of an unknown endpoint through the running UI. Physical recovery remains in
#199. The [shared topology follow-up](2026-11-08-usb-topology-recovery.md)
covers notification-driven missing-port recovery and the bounded retries
for open and firmware readback after arrival.

Dev executable SHA-256
`81e7a79a431ec1f6de204c0f02d08f27833e4d9100c2ac0480a1a26f9277a104`.
