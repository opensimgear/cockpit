# OpenSimGear cockpit

Cross-platform desktop configuration application, planned with **Tauri 2, Svelte/TypeScript, and Rust** for Windows, Linux, and macOS.

## Status

Repository scaffold initialized. The Tauri application, device access, and SimHub profiles are not implemented yet. USB HID controls work independently of the cockpit once device firmware is implemented; the application is for setup, diagnostics, and explicit updates.

## Layout

- src/: Svelte/TypeScript interface.
- [src-tauri/](src-tauri/README.md): Rust native services and serial communication.
- [integration-profiles/](integration-profiles/README.md): versioned SimHub profiles; initial integration is Windows-specific.

## Version-one scope

Configure connected modules through direct USB serial or a gateway. Discover capabilities, register/replace modules explicitly, preserve compatible settings, and require fresh calibration for replacement hardware where applicable. Use explicit Apply/Save, a single configuration writer, and clear USB reconnect notices for layout changes. Offline backup/restore is deferred.

Open device serial ports only during an explicit setup/test session and release them afterward so SimHub can access them. Firmware updates are user-initiated through direct USB, with model/hardware checks and interrupted-update recovery; a debug recovery path remains required. No automatic flashing.

Use a compatible pinned protocol release through Rust/TypeScript adapters and shared conformance vectors. Builds on all three operating systems do not substitute for testing real device access on each platform.

## Related repositories

- [Protocol](https://github.com/opensimgear/protocol): shared definitions and compatibility rules.
- [Firmware common](https://github.com/opensimgear/firmware-common): reusable C implementation.
- [Firmware template](https://github.com/opensimgear/firmware-template): starting layout for new devices.
- [Cockpit](https://github.com/opensimgear/cockpit): desktop configuration application.
- [Website](https://github.com/opensimgear/opensimgear.github.io): system architecture and user documentation.

## Licensing

License selection is pending. No license grant is implied by this scaffold. Firmware, mechanical design, and electronics will each need documented license coverage before releases and any public USB identifier application.
