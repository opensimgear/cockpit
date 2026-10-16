# Waveshare ESP32-S3-Touch-LCD-7 V1.2

This profile targets the board identified in the user's rear-board photograph: Waveshare ESP32-S3-Touch-LCD-7, Rev 1.2, 800×480. It is not the 7B or 7C variant. The V1.2 schematic specifies ESP32-S3-WROOM-1-N8R8 (8 MB flash, 8 MB octal PSRAM), ST7262 RGB display and GT911 capacitive touch. Board markings establish identity; memory, orientation and timing still need on-device verification.

## Build and flash

Use ESP-IDF **v5.5.2** and a fresh build/config directory. From apps/companion-esp32:

    idf.py -B build-waveshare-lcd7 -DIDF_TARGET=esp32s3 -DSDKCONFIG=build-waveshare-lcd7/sdkconfig -DOSG_BOARD_PROFILE=waveshare-lcd7-s3 build
    idf.py -B build-waveshare-lcd7 -p COM5 flash monitor

Replace COM5 with the board's port. Connect a data-capable cable to the USB-to-UART/CH343 connector (marked UART), not the separate native USB connector. This profile uses UART0 at 115200 baud. Flashing replaces the existing application; retain the vendor recovery firmware if needed. No erase-flash, encryption or eFuse operation is required. If automatic download fails, hold BOOT, tap RESET, then release BOOT and retry. Close monitor with Ctrl+] before running the serial helper.

The profile supplies QIO 80 MHz / 8 MB flash, octal 80 MHz PSRAM, PSRAM instruction/rodata XIP, 64 KB data cache with 64-byte lines, 240 MHz CPU and 1000 Hz RTOS ticks. It rejects insufficient PSRAM and incompatible touch identity/geometry. Use a fresh build directory when changing profiles; old sdkconfig settings override defaults. The default project profile remains headless.

No managed graphics/touch packages are downloaded. ESP-IDF owns two 768000-byte PSRAM framebuffers and two 16000-byte internal bounce buffers, plus driver allocations. The renderer itself retains its 512-byte scratch. These sizes are declared allocations, not a measured peak-memory qualification.

## Hardware configuration

| Signal | ESP32-S3 GPIO |
| --- | --- |
| RGB D0–D4 (blue) | 14, 38, 18, 17, 10 |
| RGB D5–D10 (green) | 39, 0, 45, 48, 47, 21 |
| RGB D11–D15 (red) | 1, 2, 42, 41, 40 |
| HSYNC / VSYNC / DE / PCLK | 46 / 3 / 5 / 7 |
| I2C SDA / SCL | 8 / 9 |
| GT911 INT (address selection, then input) | 4 |

RGB565 is native CPU byte order; 16 MHz falling-edge pixel clock, horizontal and vertical sync pulse 4, back porch 8 and front porch 8. CH422G configuration uses 7-bit address 0x24; its output latch is 0x38. IO1 controls touch reset, IO2 backlight, IO3 LCD reset, IO4 SD chip select and IO5 USB selection. Reset sequencing keeps backlight off and SD deselected, selects GT911 address 0x5d with INT low, then returns INT to input. SD, USB-host, CAN, RS485 and battery functions are not initialized.

The adapter writes clipped spans synchronously into the inactive framebuffer. The optional generic board present callback submits the complete frame and waits for three new VSYNC events before recycling the previous buffer. This conservatively covers a submission racing a boundary plus a complete new scanout in the pinned IDF bounce driver. Each wait is bounded to 80 ms (240 ms total). Timeout latches a display fault; potentially scanned buffers are not recycled. A full render includes this fence before the network marks the panel ready for arming.

Touch uses bounded 30 ms I2C transactions and existing 20 ms application polling. It validates GT911 product ID and its existing 800×480 geometry without rewriting factory calibration. Coordinates use vendor landscape orientation with no swapping/mirroring. Only one finger is accepted; multitouch, invalid coordinates, finger substitution without release, malformed reports and bus errors cancel input. A no-ready report retains the last state; it never fabricates release. A cached held point expires after 200 ms without a new ready report. This needs stationary-hold measurement on the actual controller; faults remain visible and require a fresh connection/arm.

## First physical test

1. Flash and inspect UART logs. Expect the Waveshare RGB565/GT911 identity message and serial-provisioning-ready status, followed by the embedded panel. No HEADLESS message should appear.
2. Check readable landscape orientation and colors. Start with large momentary buttons and simple text/numeric feedback. Keep native/game output inactive until the later recording-mode test confirms touch alignment.
3. Follow the main firmware guide to enable Cockpit's LAN listener, generate a fresh offer, provision Wi-Fi, approve installation and assign a compatible published panel. Leave --persist off for initial tests.
4. Run python tools/provision.py COM5 --name "Waveshare 7in" from the firmware directory. Complete approval/assignment while the helper waits; expect live-disarmed and the assigned live panel.
5. Tap and release once while disarmed. Startup deliberately needs an observed ready/released touch report before arming; successful I2C reads alone do not establish release.
6. Run python tools/provision.py COM5 --command arm with all fingers released. Expect live-armed and LIVE INPUT only after the host accepts the neutral baseline. Start Cockpit's separate mapping session in recording mode first.
7. Check tap, several-second hold, release and all four screen areas. A stationary hold must remain pressed without stale-touch faults. Two fingers, dragging out, explicit disarm or Wi-Fi loss must cancel input. Reconnect and explicitly arm again; old gestures must not replay.
8. Stop with python tools/provision.py COM5 --command disarm. RAM-only reboot requires fresh pairing.

If initialization fails, keep game output inactive and capture the boot log, actual flash/PSRAM detection and any vendor-demo result. No automatic alternate pin mappings, touch address probing or calibration rewrites are attempted. Physical Wi-Fi/TLS behavior, color/orientation, hold-report cadence, frame stability under traffic, peak memory/stack and latency remain acceptance work.

## Sources and provenance

Pin/timing/reset facts were checked against Waveshare's official V1.2 schematic and examples at commit bc51a84b30c0cea018fee6e0d7427ec10102060e. The adapter is original code and uses ESP-IDF APIs directly; vendor/LVGL source is not vendored.

- [Waveshare resources and controller datasheets](https://docs.waveshare.com/ESP32-S3-Touch-LCD-7/Resources-And-Documents)
- [V1.2 schematic](https://files.waveshare.com/wiki/ESP32-S3-Touch-LCD-7/ESP32-S3-Touch-LCD-7-Sch.pdf)
- [Pinned vendor RGB/touch example](https://github.com/waveshareteam/ESP32-S3-Touch-LCD-7/tree/bc51a84b30c0cea018fee6e0d7427ec10102060e/examples/ESP-IDF/08_lvgl_v8_demo/components)
- [ESP-IDF v5.5.2 RGB driver implementation](https://github.com/espressif/esp-idf/blob/v5.5.2/components/esp_lcd/rgb/esp_lcd_panel_rgb.c)

Native tests inject SDK boundaries and gate this production adapter at 71% lines, branches and functions with AddressSanitizer/UndefinedBehaviorSanitizer:

    python3 apps/companion-esp32/test/waveshare.py

They verify wiring/timings, reset/output sequencing, every initialization failure/cleanup boundary, span bounds and caller-buffer copying, frame swaps and timeout latching, touch release/hold/freshness, errors and unsupported reports. SDK cross-compilation and mocked tests do not establish physical success.
