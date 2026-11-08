# Controller visibility reader lifetime

The **Device visibility** card in controller details owns two observation
readers, `controllers.visibility` and `system.inventory`. It releases both
when the document hides or the card closes. Reveal waits for both new baselines
before reading the exact controller's status. A passive response from the
previous visibility period cannot overwrite the card. An acknowledged user
save still updates its draft when it completes during a hide.

## Running Windows Cockpit

The fixture drove the document visibility event in both clients. The previous
frontend retained its readers after document hide. The updated
native WebView and browser-dev each retired the same two reader identities
on hide, left the surrounding Devices inventory reader active, and opened two
new identities on reveal. Each made zero card status reads while hidden and
one on reveal. The browser's extra initial status read matched one committed
inventory delivery. Both fixtures verified that a saved visibility preference
changed while hidden appeared on reveal, then closed the card and restored
the preference. The fixtures used private databases and only saved local
preferences. They did not apply a hiding rule or activate a profile.

The native executable SHA-256 was
`62adc3b9a4647e31253b7519b1aedd13f1ce02a2202946601948e6ac32aa961b`.
The browser-dev gateway SHA-256 was
`6be535485b715b389568b5f2d64c8b3d5cf56b875a48250a89c3412310311a1c`.
These are the native builds from the
[aircraft catalogue verification](2026-11-08-vehicle-catalogue-wakes.md).

## Local checks

The frontend suite passed 1,707 tests. Both Svelte checks reported
zero errors and warnings. Widget type and fixture checks and the production
frontend build passed. The package check wrapper rejected the isolated
workspace's dependency junction, so the type and fixture commands ran
directly. V8 measured 88.46% statement-line and 100% function coverage of
the compiled card handlers. The shared visibility helper measured 100%
lines, branches, and functions. The component measurement excludes
generated markup and does not measure branches.

Controller report routing and physical preview latency have separate
acceptance in [#199](https://github.com/opensimgear/cockpit/issues/199).
