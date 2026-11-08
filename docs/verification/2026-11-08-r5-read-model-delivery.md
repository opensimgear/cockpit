# Read-model delivery qualification

The Windows dev Cockpit and Edge browser-dev ran against separate disposable
databases. The tested runtime and frontend source matched main at
`2446a7e1` across 1,727 tracked Rust, TypeScript, Svelte and JavaScript
files. The browser gateway was rebuilt from that source. The native
binary was the verified Windows dev build from the same revision.

| Producer | Observed in both transports |
| --- | --- |
| Discovery scan commits | Two authored scans opened the current review after navigation. The old review stayed out of the reopened view, with zero game-state refresh reads. |
| Windows process inventory | The shared read model and Active game header each delivered 20 matching transitions with zero game-state or page reads. A process at an unregistered path stayed absent. Replacing the selected executable withdrew the retired process. |
| Assetto setup files | Four writes, removals or runtime path replacements updated the exact setup view. A failed registration recovered. The fixture recorded zero state queries during file changes. |
| Prepared artwork files | Write, removal and restore delivered the expected image transitions without a fixture gateway read between changes. The retired detail reader stayed quiet, and reopen began with the current withdrawal. |
| Activity journal | One visible profile creation caused one committed-only read. The view stayed quiet for 6.5 seconds while idle and performed one baseline read on each of three reopens. A commit while Activity was closed caused no read. |
| Activity retention | One committed settings write updated the visible input and emitted one retained value after the initial baseline. The trace recorded zero settings reads during the 5.5-second observation window. An explicit read verified the trace first. |

The native AI settings fixture recorded one baseline status read and one
read for a visible credential write. It made zero reads during hidden
revocation and reopened on the current credential list. Browser-dev showed
the desktop-only boundary with zero status requests. Each transport passed
seven fixture scenarios, with the counterpart AI scenario explicitly skipped.
Thirty-two focused frontend tests also passed.

The stricter Activity fixture first reached the cold native WebView before
IPC was available. It now waits for IPC readiness before its first gateway
request. The rerun passed on native, and the browser path passed the same
exact-read assertions. Production code was unchanged.

Windows process verification used the approved shared fallback with native
events off. Granted-event consent and watcher lifetime remain in #199.
Native macOS auto-start delivery remains a platform verification item.
The integrated qualification matrix remains open in #198.

Native executable SHA-256
`0b550fa4c7acd1c196368c95d0d8335083f875a45f02feed6de33e7323285954`.
Browser gateway SHA-256
`2ae18bd682728d1a07cbb9271bcebe5f2261e1b04623fc0e8965acac3cfb310d`.
