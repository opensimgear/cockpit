# Settings reader lifetimes

The previous Windows dev Cockpit retained desktop and system inventory
readers when the Settings Devices document was hidden. The Backups pane
likewise retained its native reader. Both failures were reproduced against
the running app before changing the respective pane.

Devices Settings now opens desktop, driver, inventory, visibility and vJoy
demand readers while visible. Retirement withdraws the displayed snapshots
and fences late inventory and visibility replies. A visibility settings
commit requests the current scoped status. Desktop and driver observations
carry their own current snapshots.

Backups withdraws the catalogue and operation controls on hide. Native
operations already accepted by the gateway can finish. Their replies only
update the view that initiated them. Successful restore is a terminal
exception because it stops the observation broker. The still-mounted pane
retains the restart requirement, and a newly mounted pane can recover the
durable requirement with one status read if its opening subscription fails.

## Running Cockpit

The navigation fixture uses a private database and traces actual gateway
requests and observation lifetimes. It triggers the document visibility
transition in the running WebView and Edge, then verifies unsubscribe and
baseline acknowledgements.

| Scenario | Native WebView | Edge browser-dev |
| --- | --- | --- |
| Devices settings hidden during a saved visibility change | Zero inventory or visibility reads | Zero inventory or visibility reads |
| Devices settings reopen | One inventory read and one visibility read | One inventory read and one visibility read |
| Visible saved visibility change | One visibility read, inventory quiet | One visibility read, inventory quiet |
| Backup catalogue changes while hidden | Exact reader retired. Reopen reflected removal with zero status reads | Desktop-only notice, no backup reader |
| Reopen after queuing a restore in the private database | One status read recovered the restart requirement | Desktop-only operation |

The fixture also exercised tab navigation, external Activity retention
commits and desktop settings changes across tab reopen. The restore case
queued a restore and confirmed the restart control in the disposable
database. It left the restart command unexercised.

Native executable SHA-256
`caac4671b83f2d04168c4a8be865aa53b2483ac930de7f88d0bb6e72928deed8`.
Browser-dev gateway SHA-256
`f6dba6202283c968de62b69a87d9699ae593fec314bf8439a70e410fd85e6fcb`.
Raw logs are under `.research/reactive/device-settings-visibility/`.

## Local checks

All 1,703 portal tests passed, along with the configured shell coverage
checks, the shared visibility helper's 100% line, branch and function
coverage, Svelte checking and a production frontend build. The portal test
coverage runner measures the shared TypeScript helper. The live fixtures
exercise the changed Svelte components and their IPC boundaries.
