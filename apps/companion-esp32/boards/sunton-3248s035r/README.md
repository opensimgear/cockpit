# Sunton/GUITION ESP32-3248S035R

Profile: sunton-3248s035r. The supplied photos match the yellow 3.5-inch ESP32-WROOM-32 board with the populated XPT2046 resistive controller and four-wire resistive overlay. This profile is for the R variant, not the GT911 capacitive C variant or the smaller 2.8-inch CYD.

The native display is 320×480; the adapter exposes 480×320 landscape. ESP32 target, 4 MB DIO/40 MHz flash, no PSRAM. Only ESP-IDF v5.5.2 is used. It streams RGB565 to the ST7796 over SPI2 with a 64-byte conversion buffer (the non-DMA ESP32 SPI FIFO limit) and no full-screen framebuffer.

## Cockpit installation

Connect the board's USB data port. In Companions → Pair → ESP32, choose the unpaired USB device, expand firmware installation and select **Sunton/GUITION ESP32-3248S035R · resistive · 480×320**. Install firmware and continue, then choose direct USB or Wi-Fi using the existing provisioning wizard. Pairing/connection setup is persisted on the board; panel assignment remains controlled by Cockpit.

USB bridge identity cannot uniquely identify the screen model. Select this profile explicitly. The installer checks that the MCU is ESP32 and uses bootloader offset 0x1000, partition table 0x8000 and application 0x10000. It does not erase NVS, change eFuses or accept flash-encrypted/secured boards. If automatic download fails, hold BOOT while resetting, then retry installation.

## Driver signals

| Signal | GPIO |
| --- | --- |
| SPI clock / MOSI / MISO | 14 / 13 / 12 |
| LCD chip select / data-command | 15 / 2 |
| Backlight, active high | 27 |
| XPT2046 chip select / PENIRQ | 33 / 36 |
| SD chip select, held inactive | 5 |

Display clock 24 MHz; touch clock 2 MHz. Both devices share SPI2, serialized by a bounded mutex. Pixel writes synchronously consume CPU-order RGB565, swap to wire byte order, validate bounds and latch SPI write failures. Touch returns false on SPI failure, lock timeout or invalid ADC coordinates so the existing input monitor stops the session. Median-of-five samples and a pressure threshold reduce resistive noise. PENIRQ high reports release. Resistive panels do not provide reliable multi-finger identification.

The ST7796 uses software reset, RGB565, BGR color order and landscape MADCTL 0x28. Hardware reset is tied to the board enable/reset line. SD, speaker, RGB LED and light sensor are not initialized.

## Calibration and physical qualification

Initial raw X/Y calibration limits are 200–3900. The portrait sensor axes are rotated to landscape; raw Y maps to screen X, inverted raw X maps to screen Y. Unit tests verify the transform, not factory calibration on this particular panel. The four CONFIG_OSG_SUNTON_X/Y_MIN/MAX settings can be adjusted through IDF menuconfig for this build.

Before using real game output, check color bars/readability, the four corners and center, tap/hold/release, dragging off the panel, USB removal, Wi-Fi loss and reconnect. Physical orientation, pressure threshold and calibration remain unverified until this screen is tested. Begin with a simple panel and large momentary buttons; this board's runtime RAM is significantly smaller than the Waveshare's.

## Build

From apps/companion-esp32 with the pinned IDF environment:

    idf.py -B build-sunton-3248s035r -DIDF_TARGET=esp32 -DSDKCONFIG=build-sunton-3248s035r/sdkconfig -DOSG_BOARD_PROFILE=sunton-3248s035r build

From the repository root, stage firmware before rebuilding Cockpit:

    python3 scripts/package-companion-firmware.py apps/companion-esp32/build-sunton-3248s035r

Build success proves link/flash fit, not runtime heap headroom under Wi-Fi/TLS load. The tested image is approximately 876 KB; the configured app partition is 0x177000 bytes.

## References

- Manufacturer specifications: https://www.guition.com/icms/upload/fb081940d6fc11f09850077a33e1404f/FTPData/UEditor/file/2026121/1768961091987/ESP32-3248S035%20Specifications-EN.pdf
- Board-specific hardware configuration: https://github.com/rzeldent/platformio-espressif32-sunton/blob/main/esp32-3248S035R.json
- Controller command specification: https://docs.espressif.com/projects/esp-dev-kits/zh-cn/latest/_static/esp32-s3-lcd-ev-board/datasheets/3.5_320x480/ST7796S_SPEC_V1.0.pdf

No third-party driver implementation is bundled.

USB serial uses 115200 baud for the CH340 bridge. Cockpit detects this speed and the Waveshare 921600-baud speed automatically. If flashing cannot enter download mode, hold BOOT, tap RESET, release BOOT, select **I used BOOT / RESET** in the installer, then retry **Install firmware and continue**.
