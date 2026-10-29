# Browser companion

Cockpit serves the companion UI itself over its opt-in HTTPS listener. There is no desktop companion installation or external hosting dependency. The existing native companion code remains for Android and iOS only.

## Pair and use

1. In Settings → Companion listener, select the LAN interface/address, enable the listener, and keep its address and port stable.
2. Use **Export public certificate** to export Cockpit's public root CA certificate. Transfer it to the receiving device using a trusted channel. Verify the CA public-key fingerprint shown in Cockpit before installing the certificate in the device/browser's trusted certificate store. This does not export a private key. Certificate installation is a user/administrator action; Cockpit does not change system trust or bypass browser certificate warnings.
3. In Companions → Pair → Browser, choose **Create pairing link**. Copy the link to any supported browser that can reach Cockpit, or choose **Open in browser** on this PC.
4. Opening that fresh link automatically pairs this browser profile. The link is single-use, expires after 60 seconds, and is removed from the address bar immediately. If certificate setup took longer, create a fresh link afterward.
5. Assign a panel to the new browser companion in Cockpit. It arrives automatically; subsequent panel edits/assignments arrive without publishing or pairing again.

This is a reachable-network link, normally on the same LAN. It is not a cloud relay or a public internet share link. Host firewall/network rules must permit the chosen listener port.

Pairing credentials persist in that browser profile's site storage, while Cockpit retains the paired installation in its protected host store. The network address (including scheme and port) defines the browser storage origin: changing it, clearing site data, or using another/private profile requires another pairing. Removing the companion in Cockpit revokes the old credential. Reopening a fresh link after removal pairs a new installation automatically.

## Fullscreen and app access

Use **Fullscreen** to fill the screen; Escape exits. Fullscreen requires a user gesture and browser support. Use **Install app** when the browser offers installation, or its Install/Add to home screen menu. The manifest launches a standalone app window at the normal companion URL, without embedding a pairing link or credential. Bookmarking is the fallback.

Installed app access still requires Cockpit to be running and reachable. Listener settings are retained and restored when Cockpit starts. Browser/PWA installation does not create a background input agent or make offline input possible.

## Input lifecycle

A verified assigned panel opens a new session and sends every control's fallback value before becoming active automatically. There is no manual arming button. Inputs use the existing authenticated, ordered host protocol; they are never replayed after reconnect.

Panel transfer and rendering use a separate, bounded preparation lease while
controls remain disabled. The browser keeps that session alive during preparation;
the short active-control lease begins after its neutral baseline is accepted.
Expired sessions recover automatically and retrieve the current assignment and
feedback snapshot without replaying old inputs or asking the user to reconnect.

Losing focus clears queued input and sends every control's neutral value on the
existing connection. Telemetry and feedback continue while Cockpit or a game has
focus; browser controls stay disabled. Focusing the companion neutralizes and
validates the existing session before enabling controls. A new session is opened
only if its identity expired. Pairing and authentication alone do not grant input.

Where supported, a SharedWorker owns the socket and its 250 ms heartbeat,
independently of page rendering, focus and page timers. If the page stops sending
requests for one second, the worker releases any held controls to their declared
fallback values while continuing feedback. Requests are ordered and never
replayed. Connection-loss events start reconnection without waiting for a page
poll. Older browsers, and browsers whose worker fails to start within four
seconds, use the direct transport and its native lease protection. This fallback
only applies during startup; an established connection never replays commands
through another transport.

Unloading the page, changing page/panel or a connection failure clears queued
input and closes the old connection. Cockpit neutralizes on disconnect and its
lease still bounds recovery if the browser or worker stops. One same-origin tab owns the companion
where Web Locks is supported; the host independently enforces one writer for an
assigned instance.

An absent optional companion leaves its dependent routes inactive without marking
the whole profile degraded. Required resources and actual delivery errors still
affect profile health. A normal pause outside a desktop output's application scope
is healthy, as is waiting for the profile's game to open; the paused or waiting
status remains visible in runtime inspection.

## Build and verify

Run pnpm browser:check and pnpm browser:test. The pnpm ui:build command builds both browser companion and Cockpit. Build Cockpit native after the UI so its build script embeds apps/browser-companion/dist.

The companion-service HTTPS browser transport test covers static assets, Host/Origin admission, native protocol compatibility, one-use pairing, live assignment, input, disconnect neutralization, reconnection and revocation.

With the native development Cockpit running, `node --test
tests/ui/browser-worker-startup-native.mjs` verifies real browser pairing both
normally and with the worker download deliberately stalled. Set
`OSG_PLAYWRIGHT_MODULE` to the installed Playwright package when needed.

The browser credential is available to same-origin JavaScript rather than OS-protected native storage. The dedicated origin, restrictive CSP, no third-party scripts, exact-origin WebSocket admission, and HTTPS protect this boundary. The public web endpoint exposes no Cockpit operator commands or arbitrary files.
