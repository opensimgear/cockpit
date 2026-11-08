# Activation progress after hiding and reopening

The native Windows fixture hid the actual progress window while its
cancellable operation was running. It committed noncancellable cleanup and
revealed the same window handle. The WebView showed the cleanup message with
the Cancel button removed. A second hide and terminal commit destroyed the
popup. A new operation then opened with its own Cancel button. Commands
carrying the retired operation ID left the new popup intact.

Windows confirmed the visibility changes. The embedded WebView2 document
reported `visible` even while its native popup was hidden. This run qualifies
the Windows popup lifetime and retained progress while the popup is offscreen.

The browser-dev fixture supplied authored progress frames through the browser
NDJSON observation boundary. The running Svelte app showed cancellable
preparation, then noncancellable cleanup after reveal. A terminal frame while
hidden removed the dialog. A full page reload on a retained terminal baseline
also showed no dialog. The app presenter and a separate reader shared one
activation subscription and one transport connection. All progress deliveries
were acknowledged. Headless Edge reported `visible` during tab switches and
minimization, so this run simulated browser document visibility. The page
reload and observation delivery ran in Edge.

Both fixtures used isolated dev sessions with authored progress and no
simulator or device operation. All 2,113 frontend and companion tests passed.
Focused coverage passed with 97.44% lines, 86.92% branches and 95.24%
functions for the presenter and 93.88% lines, 85.71% branches and 89.47%
functions for the native-window adapter. The rest of the UI background and
reopen matrix remains open in #196.

The native dev executable SHA-256 was
`ec77d9bad6abd5e111ad49919b4addccbeba7161875fe9bfe93e3504cad40e37`.
