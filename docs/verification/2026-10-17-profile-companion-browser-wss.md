# Native profile feedback in an actual browser — 17 October 2026

Local acceptance evidence for [#79](https://github.com/opensimgear/cockpit/issues/79). The successful run used the built browser companion, an actual loopback HTTPS/WSS listener, the production Hub and CompanionProvider, native resource observations and sealed delivery tokens, NativeProfileHost, Coordinator, the typed reducer, and the shared PanelCanvas renderer. No browser IPC endpoint or input/feedback reply was mocked.

The rendered browser toggle is explicitly labelled **Authored altitude: 9000 / 12000 ft**. Its real WSS boolean sample selects those two authored constants in the native profile. The graph then performs feet-to-metres conversion, hysteresis with press/release thresholds of 3000/2900 m, Gate, and Format. This is an authored preset demonstration; it does not measure simulator altitude. Output provenance is `local` with source kind `host`; the proof rejects a `game-confirmed` presentation.

The test alone sets up an isolated pre-approved pairing through the Hub's actual pairing protocol and passes its issued credential to a fresh browser context. It does not fabricate a resource token or call a trusted feedback publisher to supply the result. The controller input worker is disabled, physical panel I/O is empty, no USB discovery is started, and game discovery/launch/provider I/O panic if invoked. This validates browser/native profile feedback, without qualifying physical hardware, a simulator, virtual-controller submission, or the Cockpit authoring UI.

## Observed results

All rows below were asserted against the real rendered widgets and recorded alongside native Coordinator state and WSS traffic.

| Action | Native session | Indicator | Text |
| --- | --- | --- | --- |
| Apply, neutral 9000 ft preset (2743.2 m) | Running | Inactive | Unknown; Gate withholds the value |
| Toggle 12000 ft preset (3657.6 m) | Running | Active and lit | `ALT 3657.6 m` |
| Toggle below the release threshold | Running | Inactive | Unknown |
| Clear visual state, then resync | Running | Active after resync | `ALT 3657.6 m`; no additional input frame |
| Withhold actual outgoing WSS traffic | Degraded | Stale, historical and unlit | Historical last value, visibly stale |
| Close the actual WSS connection | Degraded | Disconnected and unlit | Disconnected |
| Reconnect with a new neutral baseline | Running | Inactive | Unknown; prior toggle/value not replayed |
| Native Pause | Paused | Unavailable and unlit | Prior live value withdrawn |
| Explicit Resume | Stopped | Unknown | Unknown; no implicit Apply |
| Explicit Apply, then switch to Default | Running Default | Unavailable and unlit | Prior live value withdrawn |
| Explicit Apply, then native Stop | Stopped | Unavailable and unlit | Prior live value withdrawn |

The stale/disconnect stages alter transport availability only: the browser wrapper withholds outgoing real requests or closes real sockets. It never injects a replacement input, acknowledgement or feedback frame. Reapplying requires the actual Hub to observe a connected, armed, neutral browser baseline first.

## Race found and fixed

The first fresh run exposed a production takeover race. Acquiring the profile assignment revokes the previous browser session; the browser's new baseline advances the native provider generation before the periodic inventory worker captures it. An otherwise valid cached route could therefore bind the prior generation and fail activation when the native baseline was converted.

During the existing bounded native baseline acquisition, the host now captures actual resource observations after the replacement baseline is ready, rechecks the transition authority, and only then obtains the current snapshot/tokens and clock. Exact publication, paired instance, transport, provider identity and token validation remain required. The new regression disables peer-side periodic inventory refresh for 60 seconds, retains an initially valid old route, and verifies that takeover captures the advanced real generation before activation. No refresh timing assumption or invented token makes this regression pass.

## Reproduction and retained evidence

Build browser assets before compiling the native test executable; its listener serves those embedded assets. Then run the explicit local browser proof:

```powershell
pnpm browser:build
cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run --message-format=json
$env:OSG_COMPANION_LIVE_EXE = '<native test executable from compiler-artifact>'
$env:OSG_PLAYWRIGHT_MODULE = '<installed playwright module directory>'
$env:OSG_BROWSER_CHANNEL = 'msedge'
# Optional: use a new empty absolute evidence directory for each run.
$env:OSG_COMPANION_LIVE_DIR = '<new absolute evidence directory>'
node --test tests/ui/profile-companion-live.mjs
```

The driver copies the executable before launch, hides its console, owns teardown, and requests the ignored native test explicitly. Its file control accepts lifecycle actions only, never source values. The native helper has a 180-second bound. Pairing credentials stay in the private run directory, are omitted from recorded wire evidence, and are removed from the final handoff on clean shutdown.

Accepted local run: `.research/p9-companion-live/verified-run-6`, recorded at `2026-10-17T13:49:45Z`, Edge `155.0.4283.45`. Browser test **1 passed**, native helper **1 passed**, native companion host suite **7 passed / 1 explicit-browser test ignored**, native companion adapter suite **10 passed**. The browser run took 10.8 seconds; the native helper exited successfully after 10.20 seconds. No browser page errors were recorded.

- Native executable SHA-256: `3990c383dfa38dc1182820ecde68896d05e9437239b1eb95fd051ff42c98c2a0`.
- `evidence.json` SHA-256: `b6a79315a1eb30d93854cc6b20ffe8637562109a79cca5bfb27be2d127b2d8f4`.
- `authored-altitude-high.png` SHA-256: `79cda6b0f021e6e5b42db499283c9e043c7411fa524e8721c509b7d9156120fb`.

The private directory also retains the authored profile, native log, full sanitized wire observations, native state at each stage, and screenshots for low/high, stale, disconnected, reconnect, Pause, Default and Stop. The high and stale screenshots were visually inspected: the active indicator is lit only while fresh, the stale indicator is unlit, and both outputs show their real local/host provenance.
