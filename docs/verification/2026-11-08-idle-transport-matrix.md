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

## Full recheck after the profile-mode settings fix

The Windows WebView build from `c8a57cb0` and browser-dev gateway from
`ec7348da` each ran in a private database. Both used the `88ee864e` frontend
with the committed profile-mode control and settings guard. The fixture
required the mode control to become writable before tracing. Browser-dev
also required a ready workspace observation and a settled discovery review.

All eight views passed on both transports. Each of the 16 visible windows
ran for 60,100 to 60,115 ms after settling. The traces recorded zero gateway
requests and zero observation protocol requests in every window. The test
counts exclude the initial settings baseline and the native instrumentation
probe before each window. Live owner changes and writes are covered
separately. [Profile mode and active editing](2026-11-09-profile-mode-editing.md)

| Local artifact | SHA-256 |
| --- | --- |
| Windows dev executable | `4933f3a9c9bea2da4106484d24248e39f644095676473091109b7076a7ad4050` |
| Browser-dev gateway | `7cd535cca53574c3d6410027a6f808e497ab0246c18bb8732454aa573fc653ae` |
| `native.log` | `c0831ba6a5d0a7149a526c73ae96b483c91b9cfc9d61c04b9ba9ad3bdb46cd4c` |
| `native-summary.json` | `c57a089c6f5faf7d27b499a1de544035a40f2605ffe95fd8611d9ae608f103d0` |
| `browser.log` | `a564b6f19e71a3e7d6d1ed58463cd57e5fe51403357340a2b4c37e7d5d28bcc9` |
| `browser-summary.json` | `94b362905624a3022c174095e3dfb25237fff9f706946bc75d90077993a380dc` |
| Private `reactive-idle-mode-ready.mjs` | `019663f8eb39e0effcfdf918f1d0db63454cbe4ab91a2ff623a3ee1f3e46d5e7` |

The traces are under `.research/c193-reconnect/linux-companion-local/mode-idle-matrix/`.
