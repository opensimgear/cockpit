# ESP32 status and firmware updates

- Generic runtime shows pairing instructions, assignment instructions, or a disconnected message in place of the panel. Live panels have no debug heading.
- Firmware reports release version 2026.10.16.1 and its board profile in nonce-matched USB status. Cockpit retains this non-secret metadata for offline tile badges; legacy firmware without version reporting is marked for update. Wi-Fi companions need a USB connection to check their firmware.
- Companion settings offer USB firmware updates, with paired-device identification, explicit board selection for legacy firmware, BOOT/RESET instructions, progress, failure retry and cancellation. Preparation releases USB before manual download mode. Updates write only the application segment at 0x10000; bootloader, partition table, NVS credentials and host assignments remain intact.
- Native core/raster status and partial-render tests pass at 480x320 and 800x480 under ASan/UBSan; network client/lease tests pass.
- Serial Rust suite: 14 passed, including application-only update segment selection.
- Windows companion host suite: 32 passed. Svelte/browser checks: no errors or warnings.
- Sunton and Waveshare ESP-IDF 5.5.2 builds passed and were bundled in the Windows mirror. Cockpit rebuilt and restarted.
- Actual Cockpit UI: Sunton connected, firmware update badge visible; settings show update action; preparation identifies COM6, shows the board selector and manual download instructions. Cancel was tested to restore the connection.
- Physical flashing of this release is pending; no claim of on-device upgrade verification yet.
