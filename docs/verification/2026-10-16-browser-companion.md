# Browser companion verification — 2026-10-16

Implemented a browser UI embedded in Cockpit and served at the root of its existing opt-in HTTPS companion listener. Desktop pairing is replaced with Browser pairing. Opening a single-use, 60-second fragment link automatically exchanges it for a browser-profile credential, removes the fragment, and reconnects using the saved credential. Credentials never appear in the app manifest/start URL.

The browser uses the existing publication verification, renderer, bounded ordered input queue, and host session protocol. Assignments and panel revisions refresh live. Fresh connections automatically establish neutral baselines. Blur, hide, unload, page/panel changes, transport loss and timing gaps stop input; the existing host lease remains the fallback for a stalled browser. Web Locks avoids concurrent same-origin owners. A standalone manifest and explicit Fullscreen/Install controls support browser app access.

The listener admits browser WebSockets only at /socket with its exact configured HTTPS Origin and Host. Native pinned clients retain their Origin-free /companion endpoint. Static assets are embedded and matched against a whitelist. No arbitrary files or operator commands are served. Settings exports only the public root CA certificate; installing trust remains a user action. Listener configuration persists across host restarts.

Validation completed in the Windows mirror:
- Cockpit and browser Svelte checks: zero errors/warnings.
- Browser production bundle and Cockpit production bundle: passed.
- Browser storage, transfer-integrity and serialized transport tests: 3 passed.
- Companion-service suite: 70 passed before adding the browser host-reload regression; that regression and the HTTPS browser transport test then passed together.
- Native companion-host tests: 6 passed, including listener restore/disable and network pairing link generation.
- Windows Cockpit native build: passed; app restarted with the updated pairing UI.
- Actual Cockpit UI inspected: Browser tile, network-link instructions and listener prerequisite visible; certificate-export settings present.

Manual verification still required on a receiving browser/device after the user trusts Cockpit's certificate: automatic link pairing, installed-app reopening, fullscreen and pointer/touch interaction with an assigned panel. No browser certificate warning was bypassed, no root certificate was installed, and no real browser companion was added to the user's pairing records during automated validation.

The build emits existing native unused-code/deprecation warnings and a large Cockpit JS chunk warning. The repository-wide whitespace check reports a pre-existing unrelated trailing-whitespace line in .research/controller-mapping-m0/README.md.
