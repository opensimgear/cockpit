# OpenSimGear Companion (C0)

A separate Tauri 2 + Svelte 5 + TypeScript panel client for another desktop PC, Android and iOS. The renderer, portable UI document and input/session contracts can later be shared with Cockpit's visual designer. The companion does not depend on Cockpit's controller gateway, vJoy, HidHide or mapping host.

This milestone is an executable **local feasibility preview**. Start local preview, hold either momentary button while dragging the XY control, or press the directional hat. The recorder shows normalized transitions on this device. Pointer cancellation/capture loss returns the affected controls to fallback; window blur, page hiding and navigation disarm the entire preview. Native desktop focus loss and Android/iOS suspension/resumption also advance an invalidation epoch; the frontend disarms on the event or the next native state read. Preview never resumes automatically. No command sends game input, starts a network listener or accepts pairing credentials.

## Commands

From the repository root, install the workspace dependencies and run:

- `pnpm --filter @opensimgear/companion ui:dev`: browser preview on `http://127.0.0.1:1431`.
- `pnpm --filter @opensimgear/companion dev`: standalone desktop Tauri app.
- `pnpm --filter @opensimgear/companion check`, `test`, `ui:build`: local validation.
- `pnpm --filter @opensimgear/companion tauri build --no-bundle`: build the local desktop executable.
- `cargo test --manifest-path apps/companion/src-tauri/Cargo.toml --locked`: native shell compilation/tests.

The Rust shell intentionally registers only local lifecycle read/event APIs. Production CSP has no remote network origin and there are no privileged plugins.

## Candidate targets and qualification

| Target | Configured candidate | Current scope |
| --- | --- | --- |
| Windows desktop | Windows 10/11, WebView2 | Standalone panel client. Local preview; no remote host connection. |
| Android | Minimum API 29 / Android 10 | Mobile Rust entry point and shared renderer. Native SDK/device build, multitouch, lifecycle, performance and distribution still require qualification. |
| iOS | Minimum iOS 15.0 | Mobile Rust entry point and shared renderer. Requires macOS/Xcode; native build, device lifecycle, multitouch, performance and distribution still require qualification. |

Generate native projects only when the relevant SDKs are available: `pnpm --filter @opensimgear/companion tauri android init` or `tauri ios init`, then the corresponding `android dev`/`ios dev`. Generated `src-tauri/gen` content is not committed. A configured minimum does not prove that minimum OS works.

Pairing, authenticated app-owned Rust WSS transport, publication delivery, reconnection and mapping into Cockpit's virtual output belong to later C2/C3 milestones. The pairing card is deliberately unavailable. The recorder/session model does not qualify real touch hardware, native background transitions, LAN latency or app-store packaging; these checks remain follow-up qualification stories. Desktop companion support means the panel client can run on another PC; it does not add Linux/macOS host-controller support.

The C0 renderer uses the validated reference fixture with a fixed layout and spring-return bipolar XY axes. It is not an arbitrary portable-document renderer; page layout editing, other axis fallback policies and generic widget rendering belong to C1. Windows lock/sleep and native Android/iOS lifecycle behavior remain unqualified until follow-up device checks.
