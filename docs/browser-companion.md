# Browser companion

Cockpit serves the companion UI itself over its opt-in HTTPS listener. There is no desktop companion installation or external hosting dependency. The existing native companion code remains for Android and iOS only.

## Pair and use

1. In Settings → Companion listener, select the LAN interface/address, enable the listener, and keep its address and port stable.
2. Use **Export public certificate** to export Cockpit's public root CA certificate as `opensimgear-cockpit-root-ca.crt`. Transfer it to the receiving device using a trusted channel. Verify the CA public-key fingerprint shown in Cockpit before installing the certificate in the device/browser's trusted certificate store. This exports only the root CA, without its private key or the listener's server certificate. Certificate installation is a user/administrator action; Cockpit does not change system trust or bypass browser certificate warnings.
3. In Companions → Pair → Browser, choose **Create pairing link**. Copy the link to any supported browser that can reach Cockpit, or choose **Open in browser** on this PC.
4. Opening that fresh link automatically pairs this browser profile. The link is single-use, expires after 60 seconds, and is removed from the address bar immediately. If certificate setup took longer, create a fresh link afterward.
5. Assign a panel to the new browser companion in Cockpit. It arrives automatically; subsequent panel edits/assignments arrive without publishing or pairing again.

### Install the root CA for Brave on Windows

1. Export the certificate from Cockpit, then open `opensimgear-cockpit-root-ca.crt` on the Windows PC running Brave.
2. Confirm that **Issued to** and **Issued by** both begin with **OpenSimGear Companion CA**. Choose **Install Certificate**.
3. Select **Current User**, then **Place all certificates in the following store**. Choose **Browse → Trusted Root Certification Authorities**, finish the wizard, and confirm installation of your verified Cockpit CA.
4. Completely quit and reopen Brave so it reloads certificate trust. Open Cockpit's companion address or create a fresh pairing link if the previous one expired.

Brave's Chromium certificate verifier consumes roots explicitly installed in the Windows current-user **Trusted Root Certification Authorities** store. See the [Chromium certificate trust documentation](https://chromium.googlesource.com/chromium/src/+/main/net/data/ssl/chrome_root_store/faq.md#how-does-the-chrome-certificate-verifier-integrate-with-platform-trust-stores-for-local-trust-decisions).

Do not install the **OpenSimGear Companion listener** certificate downloaded from Brave's address bar as a trusted root. That is the short-lived server certificate and changes when Cockpit renews it. If the root CA is missing, trusting an old server certificate will still leave later connections untrusted.

Each Cockpit installation generates its own unique root CA locally on first native startup. No shared CA or private key is bundled with Cockpit. It stores its CA certificate and private key in protected persistent local storage. Ordinary app restarts, listener disable/enable, LAN address or port changes, and server certificate renewal reuse the same CA. Stored identity errors stop the listener instead of silently replacing the CA. Removing Cockpit's saved identity or app data creates a new identity and requires installing its new root CA and pairing again. Keep the companion address stable because browser pairing and installed apps belong to that HTTPS origin.

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

`node --test tests/ui/companion-certificate-native.mjs` checks the running dev Cockpit's exported root CA, the live listener's TLS chain, and the visible Brave installation steps. `python scripts/companion-certificate-acceptance.py --service src-tauri/target/debug/osg-gateway-service.exe` uses isolated native processes to verify first-start creation, different CAs for separate installations, and the same CA after listener renewal and process restart.

The browser credential is available to same-origin JavaScript rather than OS-protected native storage. The dedicated origin, restrictive CSP, no third-party scripts, exact-origin WebSocket admission, and HTTPS protect this boundary. The public web endpoint exposes no Cockpit operator commands or arbitrary files.

### A worker script stays pending in DevTools

Brave and Edge can leave the SharedWorker script row pending with zero bytes in
the page's Network panel even after the worker has downloaded, executed and
connected. The worker's `connected` reply is the reliable startup check. Run
`pnpm nx run native-host:native-browser` against the development
Cockpit to verify its complete HTTPS asset and worker connection without changing
pairing or the active profile. Set `OSG_BROWSER_EXECUTABLE` to a Brave executable
to check that browser. See the [measured verification](verification/2026-10-29-browser-worker-download.md).
