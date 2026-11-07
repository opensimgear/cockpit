# General Settings desktop preference lifetime

On November 7, the running Windows dev Cockpit reproduced a stale editable
snapshot in General Settings. The fixture held a real `desktop_preferences`
request in DevTools, hid the document and released the request. The tray
toggle became available while the document was hidden.

The corrected General pane requests desktop status only while its tab is active.
It withdraws its status when the document hides and rejects late reads from
that visibility lifetime. The app's revisioned preference store continues to
deliver saved tray and progress changes. The fixture saves both preferences
through the gateway while General is hidden, restores them while General is
open, and verifies the controls follow each committed value. The fixed WebView
made three desktop status reads, one on initial open, one when the document
became visible, and one when General reopened after a tab change. It made zero
reads while hidden or in response to the visible commits.

Edge browser-dev, using a separate private database, showed the native-only
notice and kept progress controls disabled. Reopening General showed the
position committed while Styling was selected. Neither fixture changed the
operating system's auto-start setting. Updates made directly in the operating
system while General remains open still need a native source notification.

The native executable from `6c5da1ba` had SHA-256
`a4a27e62a89ef8053bee92f6d0be7c2c2793bb22ddae961dd709ea0743e6d29c`.
The browser-dev gateway had SHA-256
`4e6e6f7fb4156b18f3327f1f34b3e293ebf69281e7b288ac23b42524b9e11893`.
Both ran the isolated candidate frontend.

The broader navigation fixture passed in both running transports. It checked
two navigation cycles and exact subscription retirement, plus external desktop
output settings commits and reopen. Browser-dev cancelled one in-flight open.
An earlier gateway binary lacked the vJoy demand topic. A fresh gateway build
ran out of disk space, so the final browser checks used the cached gateway
identified above. Failed-build object files were removed.

The isolated frontend passed 1,700 tests, both Svelte type checks and the
production frontend build. The UI preference coverage gate measured 100%
lines and functions and 97.49% branches across its three modules. The
Desktop Preferences component itself was exercised by the running fixture.
The direct widget type and fixture checks passed. The top-level `pnpm check`
wrapper refused the isolated checkout's linked dependency directory, so its
underlying checks ran directly.

Run `node --test tests/ui/desktop-preferences-lifetime.mjs` with
`OSG_DESKTOP_PREFERENCES_NATIVE=1` and the disposable WebView's
`OSG_NATIVE_CDP_URL`, or set `OSG_DESKTOP_PREFERENCES_BROWSER_URL` to the
browser-dev URL. The fixture restores changed preferences before disconnecting.
