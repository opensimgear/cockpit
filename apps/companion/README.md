# OpenSimGear Companion

A separate Tauri 2 + Svelte 5 + TypeScript panel client for another desktop PC, Android and iOS. C1 uses the same `@opensimgear/companion-ui` canvas and recording-only controller as Cockpit's visual designer. The app currently loads the flight and racing reference documents locally. C2/C3 will add paired delivery of published documents; there is no network listener or pairing credential entry yet.

Start local preview to use the selected panel. The arbitrary-document renderer supports momentary/toggle/pulse/repeat buttons, selectors, hats, encoders, sliders, XY axes, and text/numeric/icon/indicator/gauge feedback. Controls obey per-document ranges, fallback, centering, latching, deadzone and inversion. Local preview records primitive transitions; it never sends game input. Its feedback views remain explicitly unavailable without a typed feedback source.

Pointer cancellation/capture loss releases owned controls; window blur, hidden pages, navigation, document/page changes and Stop disarm all values and timers. Native desktop focus loss and Android/iOS suspension/resumption advance a lifecycle epoch; the frontend disarms on the event or next state read. Preview never resumes automatically.

## Commands

From the repository root:

- `pnpm --filter @opensimgear/companion ui:dev`: browser preview at `http://127.0.0.1:1431`.
- `pnpm --filter @opensimgear/companion dev`: standalone desktop Tauri app.
- `pnpm companion:check` and `pnpm companion:test`: shared contract/model/renderer validation.
- `pnpm --filter @opensimgear/companion ui:build`: production frontend.
- `pnpm --filter @opensimgear/companion tauri build --no-bundle`: local desktop executable.
- `cargo test --manifest-path apps/companion/src-tauri/Cargo.toml --locked`: native shell proof.

The Rust shell registers only local lifecycle APIs. It has no Cockpit gateway, virtual controller, HidHide, file or network plugins. Production CSP has no remote network origin.

## Candidate targets

| Target | Candidate | Qualification |
| --- | --- | --- |
| Windows desktop | Windows 10/11, WebView2 | Local standalone preview builds on the development PC. Cross-PC delivery, lock/sleep and packaging remain separate qualification. |
| Android | Minimum API 29 / Android 10 | Shared renderer and mobile entry point configured; SDK/device builds, multitouch, lifecycle, latency and distribution need native qualification. |
| iOS | Minimum iOS 15.0 | Shared renderer and mobile entry point configured; macOS/Xcode, native devices, lifecycle, latency and distribution need qualification. |

With the relevant SDK installed, generate native projects using `pnpm --filter @opensimgear/companion tauri android init` or `tauri ios init`, followed by `android dev`/`ios dev`. Generated native projects are ignored. A configured OS minimum does not establish that it works on that OS.

Pairing, app-owned authenticated Rust WSS transport, remote publication delivery, reconnect and Cockpit virtual output belong to later C2/C3 milestones. Desktop Companion support means the panel client can run on another PC; it does not enable Linux/macOS host-controller capabilities.
