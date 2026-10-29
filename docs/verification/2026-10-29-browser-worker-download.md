# Browser worker download verification

The companion worker request appeared indefinitely pending with zero bytes in
Brave's page Network panel even while the companion displayed "No panel assigned".

Against the running native development Cockpit at `https://10.13.37.42:54437`,
an HTTPS client verified with Cockpit's exported CA received HTTP 200 and the
complete 7,310-byte `/assets/connection-worker-BxEYHDai.js` body. The same worker
executed and reported its WebSocket connected about 66 ms after construction in
Edge and 75 ms in the installed Brave version.

In both browsers, a page-target CDP Network session reported
`Network.requestWillBeSent` for the worker URL but no `Network.responseReceived`
or `Network.loadingFinished`, despite receiving the worker's `connected` message.
The pending row therefore does not prove that this shared worker is still
downloading. Its WebSocket belongs to the worker rather than the page as well.

`tests/ui/browser-worker-download-native.mjs` discovers the current worker URL
from the HTML and bundle served by the actual Cockpit, verifies the complete
HTTPS body and JavaScript MIME type, then requires the real SharedWorker to
connect within the four-second startup deadline. It needs no pairing or profile
changes, so an active profile can remain running. Set `OSG_BROWSER_EXECUTABLE`
to Brave's executable to repeat the test there; the default browser is Edge.
The focused Nx target is `pnpm nx run native-host:native-browser`. The listener
must be enabled before running it.

Final regression runs passed in Edge (45 ms) and Brave (54 ms). All 64 browser
unit tests, browser type checking and the browser production build passed.
Transport coverage was 99.18% lines, 85.84% branches and 86.89% functions;
feedback coverage was 100% in all three measures.

The existing `browser-worker-startup-native.mjs` test separately covers real
pairing with a deliberately stalled worker. Pairing requires an inactive profile.
