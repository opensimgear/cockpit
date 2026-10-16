# ESP32 setup persistence

Setup defaults to committed NVS storage after approval. Records retain transport mode, Wi-Fi credentials, pinned Cockpit trust and approved installation identity/token. Temporary pairing offers, clocks, assignment, publications and armed state are excluded. The default ordinary-board NVS is not encrypted; firmware performs no eFuse operations.

Boot restores the installation. Wi-Fi time bootstrap uses SNTP before normal pinned TLS certificate checks; blocked NTP reports a clock failure instead of bypassing validation. A fresh trusted USB clock can resume an installation. Cockpit alone selects the panel; obsolete live sessions cancel input before reauthentication and never automatically rearm.

Validation:
- Native storage adapter: mode/credential round trips, excluded panel selection/offer/clock, malformed schema/string/bool, write/commit/init errors and Forget.
- Network/protocol/input suites pass, including held-input cancellation and disarmed refresh after Cockpit invalidates a session.
- 12 provisioning helper tests pass.
- UART tests pass.
- Waveshare LCD7 ESP32-S3 firmware builds with ESP-IDF 5.5.2, 42% app partition space remaining.

The storage-only milestone was subsequently integrated, flashed and exercised through the completed wizard/direct transport. See 2026-10-16-companion-esp32-usb-wizard.md for the physical verification and remaining Wi-Fi/finger-input checks.
