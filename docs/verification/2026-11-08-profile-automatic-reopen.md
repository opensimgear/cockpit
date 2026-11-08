# Automatic profile selection while Profiles is closed

The fixture runs native Cockpit and browser-dev with separate private
databases and the approved one-second game discovery fallback. It registers
two private Windows executables and saves one eligible profile for each game.
The profiles have no resources or outputs. Each process starts independently
of Cockpit while Overview is open.

For each game, the runtime and activation streams identify the matching
profile and its successful automatic activation. Overview shows the applied
profile and detected game. Profiles reopens on the active owner. When the
process exits, Overview withdraws the detected game and Automatic reports
that it is waiting for a match. The applied owner stays active on reopen.
Starting the second process switches to the second saved profile.

A separate gateway client then submits Stop while Profiles is closed. The
activation stream reports success for that exact request ID. Overview shows
no active profile and Profiles reopens with both rows available to activate.
A CDP positive control verifies that the trace detects legacy status reads.
The subsequent automatic selections, process exits, reopens and external
Stop require zero profile library, automatic or activation status requests
from the view.

Both running fixtures passed with no cleanup errors. The observed selection
times were 1.97–2.03 seconds from process launch to the current runtime
observation. These times include process discovery and profile activation.
All 1,714 portal tests passed, including the automatic selection coverage gates.

Run `node --test tests/ui/profile-automatic-reopen.mjs` against a private dev
Cockpit. Set `OSG_PROFILE_AUTOMATIC_REOPEN_NATIVE=1` with `OSG_NATIVE_CDP_URL`
for the desktop WebView, or `OSG_PROFILE_AUTOMATIC_REOPEN_BROWSER_URL` for
browser-dev. The fixture restores selection mode and removes its profiles and
games. It reports software selection behavior for authored processes.
