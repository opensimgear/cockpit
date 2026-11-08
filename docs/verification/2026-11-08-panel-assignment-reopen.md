# Panel assignment reconciliation

The panel editor observes committed companion presence and listener state
while the editor and document are visible. Checkbox edits are scoped to the
open panel and overlay its current native assignments. Save uses the existing
host revision check and merges deliberate edits into a fresh host snapshot.
When there are no edits, native preserves the current selection.

## Running Windows Cockpit

The fixture compiled an authored panel, stored it through the designer and
paired two actual browser companions over a private loopback listener.
The previous native frontend left the first checkbox clear after the host
confirmed the assignment.

The updated Windows WebView and browser-dev each showed the first assignment
while visible. After a local uncheck, the fixture hid the editor and assigned
the second companion. Both clients made zero hidden status reads. Reveal
showed the second assignment and preserved the local uncheck. Save removed
the first assignment and kept the second, and closing and reopening the
editor showed the same result. The fixture checked the committed host
instances and revoked both peers before closing the private listener.
The existing library reopen and draft-conflict regressions passed on both
clients.

Both clients used the Windows binaries identified in the
[idle trace matrix](2026-11-08-idle-transport-matrix.md).
The local logs are in `.research/reactive/companion-panel-assignment/`.
The workload used no physical companions or simulator input.

## Local checks

All 2,112 frontend and package tests passed. The loopback compiler-resource
test required local network access. Svelte checking and the production
Cockpit and browser companion frontend builds passed. The seven focused
tests reached 100% lines, 88.24% branches and 100% functions for
`panel-actions.ts`.
