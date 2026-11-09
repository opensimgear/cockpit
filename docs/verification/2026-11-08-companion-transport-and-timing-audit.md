# Companion transport and timing audit

The Windows dev Cockpit ran both companion transport checks with private
storage and loopback listeners. The browser client connected to the real
listener with a pinned TLS identity. A panel-only profile routed a button
to local feedback. Five retained feedback frames arrived in a 1.113 second
quiet window. Hiding the page released its held input without replacing the
profile owner. Reopening, replacing the assigned panel, and restoring it each
established a neutral baseline.

The Windows native service-library probe was rebuilt offline from the
current isolated source. It passed pinned pairing, three presence-to-worker
handoffs, quiet authenticated keepalives, cancellation of a stalled TLS
handshake, and Forget. Cleanup reported no errors. The UI offer auto-approves
pairing. The pending-approval path has separate production TLS tests.
[Pending pairing verification](2026-11-05-pairing-events.md)

Both portal transports also opened Debug Runtime & recovery three times.
They waited for the actual observation baseline before issuing one status
read per open. Tab exit and document hide retired the exact subscription.
The private database had no recovery incident. This verifies the reader's
lifetime and baseline behavior. Recovery from an active profile fault needs
separate evidence.

The isolated browser-companion suite passed 75 tests. Six feedback coverage
tests reached 100% lines, branches and functions. Svelte checking reported
zero errors and warnings. Seven retired-recovery tests reached 100% lines
and functions and 96.86% branches. The native probe build emitted an existing
unused-import warning in companion persistence.

An initial browser fixture launcher omitted its gateway client identity.
Cockpit rejected the listener setup before pairing. The corrected private
launcher passed. The earlier run with a pre-change native probe was replaced
by the current probe result.

The tested runtime source is `2446a7e1`. Cockpit executable SHA-256
`0b550fa4c7acd1c196368c95d0d8335083f875a45f02feed6de33e7323285954`.
Browser-dev gateway SHA-256
`2ae18bd682728d1a07cbb9271bcebe5f2261e1b04623fc0e8965acac3cfb310d`.
Native companion probe SHA-256
`21c1f0854956c8fc0013efae715e9d1ae00826fd3e471687c77fe4b4fb019fd3`.
Raw fixture logs are under `.research/reactive/r5-read-model-qualification/`
with the `acquisition-transport` and `companion-browser-live` prefixes.
Android/iOS shell and physical board qualification remain in #199, with
mobile software prerequisites in #55/#56.

## Timing audit

The committed audit now reads its checked-in tables from the same Cockpit
revision as its inventory. The working-tree historical inventory on this
machine contains unrelated edits. The historical check initially compared
that edited inventory with the pinned tables and rejected it. The committed
checks pass for all 247 historical and 464 current sites. The frontend timer
guard and its five regressions pass. The unresolved HidHide executable and
Sunton held-coordinate acquisition boundaries remain in #189.

## Private mobile session qualification builds

The Android and iOS shells can opt in to authenticated panel sessions
with the `mobile-qualification` feature in a debug build. Standard
mobile builds keep the session gate. The qualification build still
requires its native protected store. Existing pinned TLS, assignment,
neutral baseline and explicit Start checks govern session admission.
[Build commands](../../apps/companion/README.md#development-and-native-builds)
and [Tauri CLI options](https://v2.tauri.app/reference/cli/#ios-build).

The Windows companion service passed all 241 tests with the feature
enabled. The admission regression exercised desktop, gated mobile and
qualification policies with available and locked stores. LLVM measured
94.27% lines and 88.32% functions for `client.rs`. The mobile frontend
passed 35 tests, its coverage floors, and Svelte checking.

The rebuilt Windows client-library probe passed live Cockpit pairing,
three authenticated presence handoffs, stalled TLS cancellation and
Forget. Cleanup reported zero errors. The first launcher opened the CDP
target before Tauri IPC was ready and stopped before pairing. The passing
run waited for the bridge before opening the fixture.

The host and probe SHA-256 values are
`ed43c0ead1eeb6d41a06aaee13d1b36a4bbcdb409d07b30235b71863d63536ff`
and `7c3e8b101830f664de8b036fa165285f08697f6a2ef54d6223d61e2b6293d34e`.
The live result is
`.research/reactive/companion-client-io-native/1794202389565/evidence.json`,
SHA-256 `8c90233df48f0f3ee35170b33601a50567eedfa50d6c8849d344dde1006f2b5f`.

This Windows run verified the shared client library and mobile frontend.
Native Android and iOS builds, signing, LAN and device lifecycle
qualification remain open in #55, #56 and #199.
