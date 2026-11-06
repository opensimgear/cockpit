# Portal reader lifetimes

The browser-dev Devices settings tab subscribed to `settings.desktop`, but
the broker rejected the browser audience. The existing settings projection now
admits that audience. It carries enablement, scope kind and capability without
application identity. A gateway without native output reports that capability
as unavailable.

The same trace passed on the running native Cockpit and on headless Edge using
the freshly linked gateway and an isolated browser-dev database. Each run
visited six main views twice, opened and closed both relevant Settings tabs
twice, and switched three times between the profile editor's X-Plane 11 query
and X-Plane 12 settings. Each selected query held exactly one reader, each
new reader received one baseline, the editor retained its selection, and
leaving retired the traced view readers. Reopening Devices displayed the
current keyboard and mouse settings on both transports.

Native Debug readiness teardown and native and browser preference reopen
checks also passed. Stopping only the isolated browser gateway faulted the
old reader and reopened exactly one connection with a fresh service session.
The local navigation suite passed 54 tests. The 27 shared
transport tests passed with 98.02% line, 92.53% branch and 91.07% function
coverage. The fixtures registered local executables for catalogue selection
only and did not launch X-Plane or send desktop output. The native trace used
the preceding native executable and workspace dev frontend. The changed
audience was exercised in the rebuilt browser gateway and the instrumented
native suite. Other nested surfaces and the complete background-change matrix
remain open under #196.

The local native suite passed 2,357 tests with 18 existing ignores. The
desktop settings observation reached 21 of 21 production lines. Seven
output settings UI tests and all 247 timing dispositions passed.
