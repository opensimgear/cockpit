# Debug runtime fault and recovery on reopen

The Windows dev desktop and Edge browser-dev each ran with a private Cockpit
database. The fixture applied a temporary profile with a required vJoy
output and confirmed ownership of slot 1. It opened Debug Runtime & recovery
on the active profile, then navigated to Overview.

The fixture terminated the exact vJoy feeder child of its native Cockpit or
browser gateway. It verified the executable path, parent and process start
time before termination. While Debug was closed, the retained profile
runtime stream reported the feeder loss and vJoy released the slot. Debug
reopened with the exact native fault in a single banner and an enabled Recover
action. An independent recovery request while Debug was closed cleared the
fault. The reopened view showed no active profile and no fault banner.

Both runs made zero legacy profile or activation status reads after a
positive trace control. The first native attempt used the UI gateway wrapper
for recovery. That opened its progress window, which made one status read.
The final fixture uses the raw gateway boundary for the independent request,
so the test covers recovery initiated outside the portal.

All 1,714 frontend tests and the 43 focused runtime, UI and read-model tests
passed. Runtime controller coverage was 100% lines, 95.07% branches and
100% functions.
The binaries were built from `d79b1b94`, which has the same tracked native
source as the fixture's base `ffafbda0`. Native executable SHA-256
`df55764456781ae8f19152d0b066c0f8f3c2c6125443801b1353569a31b7fd42`.
Browser gateway SHA-256
`e785ba05846f5ee4fd838efa3b6c1eaaee8cafedfa361b4c17a9a81d392d6283`.

Run [the fixture](../../tests/ui/profile-runtime-fault-reopen.mjs) against a
private Windows dev session with a compatible free vJoy slot. Set
`OSG_PROFILE_FAULT_EXECUTABLE` to the gateway executable's full path and
`OSG_PROFILE_FAULT_ROOT_PID` to the native Cockpit PID or the browser-dev
Vite PID. Set `OSG_PROFILE_FAULT_BROWSER_URL` only for the browser run.
