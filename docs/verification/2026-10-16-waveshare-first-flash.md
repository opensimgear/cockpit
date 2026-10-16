# Waveshare first physical flash — 16 October 2026

Issue #63 remains in progress. This records physical USB/flash/boot evidence, separate from visual, network and input acceptance.

- Installed usbipd-win 5.3.0 on Windows with automatic restart disabled. WSL Ubuntu is running WSL 2 with kernel 6.18.33.2.
- Shared and attached the Waveshare CH343 USB serial bridge (VID:PID 1a86:55d3). Ubuntu exposes it as /dev/ttyACM0 and a stable /dev/serial/by-id path. A device-class udev rule grants the existing plugdev group read/write access without root.
- esptool identified ESP32-S3 revision v0.2, 8 MB embedded PSRAM and **16 MB physical flash**. This unit therefore has more flash than the N8R8 device named in the vendor schematic. Secure boot and flash encryption are disabled; no eFuses/security configuration were changed.
- Saved the complete original 16777216-byte flash image, with SHA-256, under the ignored .research/waveshare-device directory before flashing. The backup has mode 0600 and is not included in source control.
- Flashed the Waveshare profile over the passed-through UART bridge at 921600 baud. esptool verified written-region hashes and reset the device normally. The current profile intentionally uses an 8 MB image layout on this 16 MB part; the bootloader reports and uses that smaller header size.
- Captured a fresh physical boot: 8 MB octal PSRAM detected at 80 MHz, PSRAM memory test passed, RGB565 800×480/GT911/two-frame board initialization succeeded and serial-provisioning-ready was emitted. No LCD initialization/presentation failure or panic appeared during the ten-second capture.
- Refreshed the Windows development mirror with 707 tracked/nonignored source files, including apps, packages, firmware, Rust crates and packaging resources. Installed its frozen pnpm dependencies and rebuilt/relaunched the native Cockpit app. A duplicate serde entry was removed only in the mirror to permit compilation; the unrelated shared source Cargo.toml was preserved. The native dev build completed successfully with warnings.

Local evidence is in .research/waveshare-device/{backup.log,before-cockpit-2026-10-16.bin,before-cockpit-2026-10-16.sha256,flash.log,boot.log}. The flash backup may contain previous installation data and must remain private. Windows mirror build logs are waveshare-tauri.stdout.log and waveshare-tauri.stderr.log in its temporary development directory.

Visual orientation/color quality, touch alignment and stationary holds, actual Wi-Fi/pairing/live panel delivery and game/backend output are **not established by these boot logs**. Private Wi-Fi entry and the host's explicit pairing/assignment flow are operator steps. No physical screen/input acceptance checkbox is marked complete.

To reattach after unplugging, use usbipd list in Windows to find the current CH343 bus ID, then usbipd attach --wsl --busid BUSID. The one-time bind is already installed. Check the Linux serial path before flashing or provisioning. Detach with usbipd detach --busid BUSID to return the port to Windows. A bus ID may change when moving USB sockets.
