# Windows auto-start registration observations

The pinned auto-start plugin checks both user and machine Run registration and
StartupApproved values. The Cockpit desktop owner arms one Windows registry
notification per key before reading the plugin status. It rearms on change and
uses a watched ancestor if a key is missing. A repeated status stays quiet.
The worker blocks between notifications and stops on desktop shutdown.
General Settings reads the retained topic while visible. Reopening General or
selecting Retry can replace a failed worker. The MCP resource exposes the same
read-only status. [Windows registry notifications](https://learn.microsoft.com/en-us/windows/win32/sysinfo/retrieving-data-from-the-registry)

The running Windows dev fixture used a random registration name and a no-op
command. An external Run value made the auto-start switch turn on. An external
StartupApproved override turned it off while the Run value was still present.
MCP listed the resource and returned the same states using a temporary read
credential. The portal made zero desktop status requests during those changes.
Switching tabs retired the exact reader. Removing the override while General
was hidden appeared in a fresh baseline when it reopened. The ordinary
disable control removed the test value. The fixture restored the private MCP
settings and left the real Cockpit startup entry untouched.

The native fixture also passed a missing private registry key, creation,
value change, deletion and recreation. The worker tests cover retained failure,
replacement of a finished worker, shutdown, and the UI/MCP audience boundary.
The existing General Settings fixture passed in the Windows WebView and Edge
browser-dev. Browser-dev keeps auto-start controls desktop-only. macOS and
Linux external auto-start changes are still only read on visible open.

The Windows dev executable SHA-256 was
`d30f3418bf051ad4aa3b0bf837a14c2233b74e1610926a9bbe5890ee1b2c1fc0`.
After a full library build ran out of disk space, the isolated qualification
build used only the Rust library output and omitted its debug symbols.

The local Windows suite passed 2,417 tests with 18 default ignores. Both pinned
DCS archive tests passed separately. The three auto-start tests passed again
after adding the duplicate-owner assertion. Production line coverage was
85.71% for the registry watcher, 94.44% for the observation owner and 100% for
MCP URI admission. LLVM module function coverage including tests was 95%,
71.43% and 100%, respectively. This instrumentation emitted no branch data.
All 1,700 frontend tests, the UI preferences coverage gate, Svelte checking
and the production frontend build passed.

To repeat, build with `reactive-qualification` and launch a disposable Windows
dev Cockpit with a fresh `OSG_AUTOSTART_QUALIFICATION_ID` UUID and a private
database. With the same UUID and its `OSG_NATIVE_CDP_URL` in the test process,
run `node --test tests/ui/desktop-autostart-observation-native.mjs`.
