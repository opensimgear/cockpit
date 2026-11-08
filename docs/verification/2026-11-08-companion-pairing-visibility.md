# Companion pairing while the Cockpit is hidden

The browser pairing countdown now follows the retained offer's expiry while
the view is visible. It schedules only the next displayed second or expiry
and stops when the window is hidden. When the view returns, it clears an
expired link immediately. Automatic browser renewal waits for a current
companion roster. A pairing that completed while hidden can therefore close
the form before a replacement offer is sent. The native host owns the offer
and its approval, consumption and expiry.

The running fixture uses a private loopback listener and real pairing commands.
It shortens only the first offer's displayed lifetime in the portal response.
The previous native and browser-dev views each sent a second pairing command
while hidden. The changed view sent no status reads or pairing commands while
hidden. On reveal the fixture held the new status response and verified that
the expired link cleared without a replacement. After releasing the response,
the view created exactly one new link. Both transports retired the delayed
roster reply from the earlier visibility cycle and restored the listener.

All 1,714 portal tests passed. The focused pairing and visibility helpers
reached 100% line and function coverage and 90.91% branch coverage together.
The Svelte check reported zero diagnostics and the frontend build passed.

Run `tests/ui/companion-connections-visibility.mjs` against a private running
dev Cockpit using `OSG_COMPANION_CONNECTIONS_VISIBILITY=1`,
`OSG_NAVIGATION_PRIVATE_LISTENER=1` and `OSG_NAVIGATION_ISOLATED_DATABASE=1`.
Use `OSG_NATIVE_CDP_URL` for the native target or
`OSG_COMPANION_CONNECTIONS_BROWSER_URL` for browser-dev. The fixture does not
redeem the links or change firmware.
