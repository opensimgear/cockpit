# OpenSimGear Companion

A separate Tauri 2 + Svelte 5 + TypeScript panel client for another Windows PC, sharing Cockpit's `@opensimgear/companion-ui` renderer. It renders immutable published C1 documents and typed feedback, or runs the flight/racing templates in recording-only local preview. Browser preview has no native pairing or remote-input access.

## Windows pairing and use

1. In Cockpit, publish a panel, enable the Companion service on a chosen LAN interface/port, and create a pairing offer. Transfer its payload directly to this Companion; nearby-host discovery is an untrusted hint, not an authorization source.
2. Paste the manual/decoded QR payload, name this installation, and request pairing. Rust verifies the out-of-band host CA SPKI pin and the ordinary TLS chain, hostname, validity and signatures before sending the one-time secret. Approve the pending request in Cockpit and assign a published controller instance. Distinct installations receive separate credentials.
3. Refresh assignments if needed, choose an instance and Connect. The exact canonical publication and bounded raster assets are verified before rendering. A fresh neutral baseline must be acknowledged before Start panel is available. Start is explicit; connecting never resumes input automatically.
4. Stop or Disconnect when finished. Window focus loss, hidden/unloaded pages, native lifecycle invalidation, a stalled UI lease, lost transport, malformed input or queue overflow stop the session and clear its source. A busy instance requires an explicit host-side Disconnect/Takeover decision. Game confirmation requires a real feedback source; a source-accepted acknowledgment alone does not claim virtual output or a game action.

The native client stores credentials with Windows user DPAPI; tokens never enter JavaScript. Only two application commands are allowed to the trusted local main window: lifecycle state and a narrow Companion client request API. Remote navigation is denied. There are no JavaScript network, filesystem, shell, Cockpit gateway, vJoy or HidHide permissions. The WSS endpoint exposes only the Companion protocol.

Digital transitions use ordered IPC and ordered native WSS input batches. Each batch has at most 64 changes; the frontend and native queue each cap pending/in-flight transitions at 256 (512 combined), reject backlogs older than 250 ms and stop on overflow rather than replaying stale controls. Input is limited to 120 batches/second. The native 600 ms UI lease and host 1500 ms source lease remain independent of renderer callbacks. Normal fallbacks for all 128 primitives are split into legal batches before close.

Local preview records control transitions only. Its shared renderer supports buttons, toggles, bounded pulses/repeats, selectors, hats, encoders, sliders, XY axes and typed text/numeric/icon/indicator/gauge output. Per-document ranges, fallback, centering, latching, deadzone and inversion apply. Cancellation releases owned pointers; Stop, page/document changes and lifecycle loss clear all timers and latched values. Preview never resumes automatically.

## Commands

From the repository root:

- `pnpm --filter @opensimgear/companion ui:dev`: browser preview at `http://127.0.0.1:1431`.
- `pnpm --filter @opensimgear/companion dev`: standalone desktop Tauri app.
- `pnpm companion:check` and `pnpm companion:test`: contract/model/renderer/storage and client queue validation.
- `pnpm --filter @opensimgear/companion ui:build`: production frontend.
- `pnpm --filter @opensimgear/companion tauri build --no-bundle`: local desktop executable.
- `cargo test --manifest-path apps/companion/src-tauri/Cargo.toml --locked`: native shell/lifecycle/navigation tests.
- `cargo test --manifest-path crates/companion-service/Cargo.toml --locked`: core, TLS and real-WSS isolation/lifecycle regression tests.

## Targets and qualification

| Target | Candidate | Status |
| --- | --- | --- |
| Windows desktop | Windows 10/11, WebView2 | Native pairing, sealed credential reload and multi-client sessions covered by local Windows WSS tests. Cross-PC LAN/firewall, discovery, lock/sleep and extended-load qualification remain in #60. |
| Android | Minimum API 29 / Android 10 | Shared renderer and mobile entry point configured. Pairing/authentication is disabled until protected storage and native-device qualification in #55. Local preview remains available. |
| iOS | Minimum iOS 15.0 | Shared renderer and mobile entry point configured. Pairing/authentication is disabled until protected storage and native-device qualification in #56. Local preview remains available. |

With the relevant SDK installed, generate native projects using `pnpm --filter @opensimgear/companion tauri android init` or `tauri ios init`, followed by `android dev`/`ios dev`. Generated projects are ignored. A configured minimum OS version or a Windows build does not establish mobile support. Linux/macOS host-controller capabilities and ordinary Cockpit live-output qualification gates are unchanged.
