# X-Plane acquisition lock scope

The shared X-Plane registry now serializes owner setup by exact process
identity. Catalogue loading and retiring a failed owner for one process
leave unrelated process scopes available. Readers for the same process
still share one owner for an identical setup. A competing setup for that
same process still requires the live owner to retire.

The native regression holds one owner at its catalogue-open boundary,
opens a different process through the same production registry and checks
that it receives a different UDP socket before the first open is released.
Existing registry tests cover one factory for identical concurrent opens,
competing setup refusal, and recovery from a failed owner.

The Windows native suite passed 2,460 tests with 18 default ignores.
Both changed files passed the repository's 71% production line gate,
with 95.95% for the owner and 90.34% for its client. All 20 named
production functions were exercised. LLVM's supplemental function counts,
which include compiler-generated closures and test helpers, were 60.00%
for the owner and 84.62% for its client. The build emitted no branch counts.

The rebuilt Windows Cockpit passed the X-Plane 11 and 12 setup fixture.
Private SDK writes updated the subscribed catalogue revisions without
fixture gateway reads. The fixture used the installed SDK definition file
and private installations. Executable SHA-256
`75278e36d9a3964784ad9feb00ae3f84c3c582730e04e93b4830c30248fd8964`.
The contention test uses authored UDP sockets. Live simulator telemetry
and Windows process-monitor consent remain in #199.
