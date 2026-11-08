# Companion status requests follow view lifetime

The panel editor and companion roster share the gateway queue with explicit
companion writes. A status read requested while a view was visible could
still be waiting behind a write when the view hid. The transport now checks
the same view lifetime before dispatch that the consumer uses to validate
the reply. Retiring the view withdraws a queued read. A new visible scope
can request a fresh status baseline.

The gateway regression holds an assignment write in flight, queues a status
read, hides the owner, and releases the write. The transport receives the
write and the later visible status request. A browser case also verifies a
retired read never reaches fetch.

## Running dev Cockpit

The native WebView and Edge browser-dev used the same frontend candidate and
separate disposable databases. Four checks passed in each transport. They
covered roster visibility, library reopen, preservation of open panel and
widget drafts, and assignment reconciliation. Both assignment traces
observed zero status reads while hidden. A deliberate local change survived
an external assignment, and reopening showed the saved selection.

The first native harness attempt needed the bundled Playwright module path.
Once the fixture could run, its pre-fix trace recorded one hidden status
request. The final native and browser-dev runs used the same bundled
Playwright configuration.

Native executable SHA-256
`742d61d956bf1f23205159e2b9571cffa0effcdc347b17a93f4b173331c1724d`.
Browser-dev gateway SHA-256
`4f306d5c06ef1d8dc7fb6e63c4d02af9daa10536e65ea9858d0e1cc9c39c0b2c`.

All 1,711 portal tests passed. The Svelte check reported zero diagnostics
and the production frontend build completed. Companion settings, panel
actions and visible-observation helpers reached 100% line and function
coverage and 88.24–100% branch coverage.
The 44 gateway tests measured the exact TypeScript-transpiled gateway at
99.50% lines, 96.11% branches and 89.29% functions by giving the existing VM
fixture a coverage filename.
