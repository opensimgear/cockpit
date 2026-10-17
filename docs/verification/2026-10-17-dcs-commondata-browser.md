# DCS-BIOS CommonData through the native profile and browser — 17 October 2026

The accepted local run exercised actual loopback DCS-BIOS UDP decoding, the full checksum-pinned release catalogue, native resource observations, the typed compiler/reducer, NativeProfileHost and Coordinator, the production Hub HTTPS/WSS listener, and the shared PanelCanvas in Edge. The browser displayed feedback produced by the native graph. No browser IPC was intercepted, no input or feedback frame was injected, and no WebSocket implementation or traffic was replaced.

This is an assembled **software protocol-fixture proof** for [#79](https://github.com/opensimgear/cockpit/issues/79). A test Platform supplies the explicitly labelled process identity; a native sender emits fixed official-format UDP fixtures. No DCS process or Lua exporter runs. No gateway service, USB/HID discovery, physical panel, virtual-controller backend or game binding is exercised. Serial enumeration, physical panel I/O and game launch are disabled or panic if invoked. Output provenance is `local`, source kind `host`; the driver rejects `game-confirmed` feedback.

## Quantity and source contract

The source is the actual pinned CommonData port `channel:common:CommonData:ALT_MSL_FT:0`: unitless integer `0..65535`, with the production-default 1500 ms freshness policy. The native helper extracts the complete `A-10C.json`, `AircraftAliases.json`, `MetadataStart.json`, `MetadataEnd.json` and `CommonData.json` documents from DCS-BIOS v0.11.7. It does not substitute a small catalogue or invent a unitful altitude descriptor.

The graph explicitly interprets the raw integer as **Reported MSL altitude**, in feet, with DeclareQuantity. That operation preserves the numeric schema and value. UnitConvert then converts feet to metres, followed by hysteresis at 3000/2900 m, Gate and Format. The initial fixture progresses aircraft metadata and completed-export counters while omitting all altitude bytes, so the applied raw and declared outputs begin Unknown. The first low fixture supplies the first altitude value.

“Reported” is material: the upstream exporter uses a legacy `0.304` divisor and does not advertise measurement validity. It can retain a previous altitude when ownship data is unavailable while export counters continue. This proof does not establish physical altitude accuracy, detect that ownship-export condition, or certify a real simulator measurement. Zero remains a valid raw value, not an absence marker. The native handoff records `measurementValidityAdvertised:false`, `actualGame:false` and `simulatorTelemetry:false`.

## Observed results

Each retained stage includes the actual Coordinator inspection cache, native session state, rendered widget state and a full-page screenshot. Values below are reported fixture values, not browser-authored input.

| Stage | Actual native observation | Rendered indicator and text |
| --- | --- | --- |
| Before Apply | No owner or live reducer inspection | Unknown; unlit |
| Apply before any altitude bytes | Raw, declared feet and metres Unknown | Unknown; unlit |
| 9000 reported ft | Raw and declared `9000`; `2743.2` m | Inactive; text Unknown because Gate is closed |
| 10000 reported ft | Raw and declared `10000`; `3048` m | Active and lit; `ALT 3048.0 m`, local/host |
| 9700 reported ft | `2956.56` m; hysteresis remains true | Active and lit; `ALT 2956.6 m` |
| 9500 reported ft | `2895.6` m; hysteresis becomes false | Inactive; text Unknown |
| No further UDP | Raw, declaration and conversion become Stale | Stale, unlit; no current value |
| Fresh low baseline | Raw `9000`, fresh conversion | Inactive; text Unknown |
| Frozen completed-export counter | UDP packet count increases; unchanged counter; actual cache becomes Stale | Stale, unlit; no current value |
| Wrong-aircraft metadata | `F-16C_50` does not match selected A-10C; raw cache Stale | Stale and unlit |
| `NONE` aircraft metadata | Raw cache Stale | Stale and unlit |
| Injected process context disappears | Actual cache Unavailable | Disconnected and unlit |
| Explicit Stop, context restoration and new Apply | New owner and a fresh low baseline | Inactive; no prior warning replay |
| Native Pause | Paused; feedback withdrawn | Disconnected and unlit |
| Resume | Stopped; no implicit Apply | Prior feedback remains withdrawn |
| Explicit Apply, then Default | Running Default | Prior feedback withdrawn; unlit |
| Explicit Apply, then Stop | Stopped | Prior feedback withdrawn; unlit |

The stale/frozen assertions use the native inspection cache, rather than only the browser's visual ageing clock. Wrong-aircraft and `NONE` are actual decoded metadata changes; process loss is the injected Platform transition. None claims real-game process or ownship validation. The browser control is labelled **Unconnected browser control (not altitude)**; the driver supplies no high input sample. File control accepts only fixed fixture/lifecycle action names and an acknowledgement sequence, never numeric telemetry, a resource token or an ownership receipt.

## Reproduction

Build the browser assets before the native test executable, because the listener embeds them. Use a normal uninstrumented native build and a new empty private evidence directory. Loopback UDP port 5010 must be free; the helper refuses to share an existing owner. It does not launch another gateway service.

```powershell
pnpm browser:build
cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run --message-format=json
$env:OSG_COMPANION_DCS_LIVE_EXE = '<native executable from compiler-artifact>'
$env:OSG_DCS_SETUP_ARCHIVE = '<checksum-pinned DCS-BIOS_v0.11.7.zip>'
$env:OSG_PLAYWRIGHT_MODULE = '<installed playwright module directory>'
$env:OSG_BROWSER_CHANNEL = 'msedge'
$env:OSG_COMPANION_DCS_LIVE_DIR = '<new empty absolute evidence directory>'
node --test tests/ui/profile-companion-dcs-live.mjs
```

The driver copies the native executable, hides its console, requests only the explicit ignored test `profile_session_host::companion::tests::live::dcs_live::actual_browser_wss_dcs_commondata_feedback_proof`, and owns teardown. The native helper has a 240-second bound. The browser uses an actual Hub-issued pairing credential in a fresh context. Credentials and native LLVM profiles remain in the private run directory; recorded wire frames omit authority tokens and transferred asset bytes. Clean shutdown removes the credential from the final handoff.

## Retained evidence and limitations

Accepted run: `.research/p5-dcs-browser/verified-run-3`, recorded at `2026-10-17T19:29:26.742Z`. Browser test **1 passed** in 40.2 seconds; native helper **1 passed** in 38.78 seconds. The evidence retains **17 stages**, 264 sent and 264 received sanitized WSS frames across eight real connections, and zero browser page errors. Edge was `155.0.4283.45` (`msedge`), Node `v24.19.0`. High and frozen-counter screenshots were visually inspected: the high indicator is lit with `ALT 3048.0 m` and local/host provenance; stale feedback is unlit and has no current value.

- Native executable SHA-256: `662223512bcccef08cd37052b2d3bff773d522dda51a1c0f6613b50d87f25692`.
- Browser driver SHA-256: `fb39ad049355cf1ba173d822783c3d34a891aa0c282fc7efeff89dda30155a68`.
- Pinned archive SHA-256: `3272bc07210fa6c560f115cfb5dc8881accf7af5e7a5e218e2c46e1fd89e3292` (8,234,988 bytes).
- `evidence.json` SHA-256: `d515fdb57092afa27d7bde25682e5bf4a57c3b02dbe94663a36b393c1f153ff4`.
- `reported-high.png` SHA-256: `0a8195eb20cdd428787a5d4b36f042a3a11081ca60e9fe61221795a54d3576fc`.
- `frozen-export-counter.png` SHA-256: `55cb948fe2b5806e8dafc2e765c7993bcec16875c3e0b8ff273752096fe8e0e2`.

`.research/p5-dcs-browser/normal-build-manifest.json` records 373 source hashes and no changes during the build, together with the actual Cargo JSON artifact in `native-build-normal.jsonl`. The driver also saves its own exact source copy and verifies its hash stayed unchanged during the proof. The run directory retains the authored graph, actual observed resource/catalogue inventory, native log, inspection snapshots, sanitized WSS observations and screenshots.

Two failed coverage-instrumented attempts are preserved in `verified-run-1` and `verified-run-2`. Their first Apply safely rejected with `The game baseline expired before command admission`, compensated effects and left RecoveryNeeded; no later telemetry stage was accepted. Run 1 used a test-specific 250 ms provider limit. Run 2 used the production-default 1500 ms limit and the same source as the successful normal build. The generic error also covers context loss, so these attempts establish safe rejection and a build-mode comparison, not the precise internal timing or invalidation cause. No admission guard was weakened, no production timeout changed, and no Apply retry was added. A successful ignored native helper exit in those failed attempts reflects teardown, not a successful browser proof.

This supplements the separate [authored-toggle WSS proof](2026-10-17-profile-companion-browser-wss.md) and [complete-profile authoring proof](2026-10-17-complete-profile-authoring-ui.md). Actual DCS/Lua execution, Windows game detection, ownship validity, real devices, controller publication/reconnect, virtual-driver submission and game-confirmed results require their separate qualification evidence.
