# Passive vehicle process and setup wakes

The passive vehicle worker now waits on native capture and exact process
notifications, library and setup commits, and its nearest source deadline.
This replaces the 100 ms vehicle process qualification loop. An admitted
reader needs a live process watch and a native capture registration.
When either registration fails, Cockpit reports the failure and the context
owner schedules a reader-open retry. The shared Windows game discovery
fallback keeps its approved one-second period.

Installation and committed setup changes wake the vehicle worker independently
of catalogue work. The scope check includes the selected installation and
process candidates. Cached setup readiness can invalidate a reader at the same
provider settings revision. An unchanged quiet reader keeps its capture and
performs zero provider reads during a scope check. Detached work must still
match its generation and configuration before publishing. A DCS capture
watch replacement releases the previous registration before registering the
new one.

## Running Windows Cockpit

The authored DCS fixture kept its test process and packet producer alive,
removed the managed exporter, and observed withdrawal of both BIOS and
ownship telemetry. Restoring the exporter recovered both families at the
same setup revision. Native WebView and browser-dev also passed competing
process withdrawal and recovery, independently expiring source families,
and process exit. Both reported zero cleanup errors.

The native executable SHA-256 was
`92f17b6a8efb7091736ad974de96b984b4eaffd58c3cca7054481b96eec6aa60`.
The browser-dev gateway SHA-256 was
`db459a863fc3812ba8743520f5ab9a4a3e9bc55c6246a9919b9aaa3e1f35019f`.
The fixture evidence is in
`.research/dcs-physics-native/1794108825375/evidence.json` and
`.research/dcs-physics-native/1794108901566/evidence.json`.

Both clients also passed the Assetto setup outage, recovery and nested
runtime replacement fixture with zero setup state queries. Focused tests
used a monitored X-Plane fixture and retained Assetto process evidence.
The running tests used authored sources. Operator permission and real
simulator qualification for X-Plane and Assetto remain in #199.

## Local checks

The final Windows native suite passed 2,429 tests with 18 default ignores.
Two pinned archive tests passed separately. All 95 focused native tests
passed after the DCS setup-cache fixture was given the explicit integration
binding used in production. The six changed native production modules
measured 92.76–98.88% line coverage and 96.88–100% declared source function
coverage. The raw LLVM function total for `setup_cache.rs`, including
generated closures, was 63.89%. This build provided line and function
instrumentation. The timing inventory matched 453 current and 247 historical
sites.
