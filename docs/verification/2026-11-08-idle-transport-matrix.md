# Idle gateway traffic on native and browser-dev Cockpit

The Windows native WebView and browser-dev gateway ran against the source
at `5653848b4a5ce09aabb7a0b04e154b41002a4844` in separate private databases.
Both loaded the same isolated frontend. All 617 frontend source,
configuration and asset records compared with main matched.

The browser startup fixture now subscribes to `games.discovery` as well as
`games.state` and `games.jobs`. The first run timed out before its measurement
because the summary stream had already been split from the discovery review.
The corrected fixture observed the ready review and matching job, then
released its startup readers. It also checked that each browser view was
visible at both ends of its measurement window.

The fixture allows legitimate editing-claim renewal and logs it separately
from state acquisition. Each row below counts all gateway requests observed
after the initial view settle interval. Native probe requests used to check
the CDP instrumentation run before each measurement.

| View | Native ms | Native gateway requests | Browser-dev ms | Browser-dev gateway requests |
| --- | ---: | ---: | ---: | ---: |
| Overview | 60,104 | 0 | 60,108 | 0 |
| Devices | 60,104 | 0 | 60,113 | 0 |
| Profiles | 60,108 | 0 | 60,108 | 0 |
| Games | 60,111 | 0 | 60,113 | 0 |
| Companions | 60,112 | 0 | 60,102 | 0 |
| Activity | 60,102 | 0 | 60,109 | 0 |
| Debug | 60,107 | 0 | 60,106 | 0 |
| Settings | 60,102 | 0 | 60,103 | 0 |

Both clients also issued zero observation protocol requests during the 16
measured windows.

Native executable SHA-256
`2e8d87fb0591cf0f1947ca8a4e5a46a30fa7ce0047f3e7913dcb850db19ad915`.
Browser-dev gateway SHA-256
`cd73a4ce68c7ba402b60ae3513072a4216f5f09fb38cfbee1b02b1f54edd808b`.
Raw logs are in `.research/reactive/idle-main-5653848b/`.

The 32 game read-model tests passed with the existing 71% coverage floor
for lines, branches and functions. The audit accounts for all 457 current
timing sites. These traces cover the top-level views while idle. Integrated
live workloads and physical latency remain separate acceptance checks.
