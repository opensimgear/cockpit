# Cockpit ESP32 USB provisioning and direct transport

Implemented native USB enumeration and a persistent serial worker, an ESP32 setup wizard, Wi-Fi scanning/password setup, scoped automatic pairing approval, optional initial Cockpit assignment, direct USB protocol delivery, session-bound explicit arming, disarm/reconnect/forget and Settings listener controls. Display and touch interfaces remain board independent.

The host generates the installation identity and approves only that setup action's matching pending request. USB link IDs are correlated with ordered requests and mapped to freshly generated native connection identities. Publications use bounded ordered SHA-256 checked chunks. Driver writes use 921600 baud. Pairing/authentication allow cold protected-store operations three seconds; live heartbeat and input safety deadlines remain unchanged. Credentials persist in ordinary NVS; assignments/publications/clocks/arming do not. Ordinary NVS is unencrypted on this board.

Validated on the physical Waveshare ESP32-S3-Touch-LCD-7 V1.2 through COM5 using the real native GatewayManager:
- Protocol-v2 USB readiness and Wi-Fi scan (multiple runs, 12–15 visible networks).
- Direct pairing, matching automatic approval, Cockpit assignment and live fixture panel delivery.
- Approved installation saved on the board.
- Explicit neutral baseline/arming and continued healthy leases.
- Assignment refresh from Cockpit establishes a fresh disarmed session without persisting a panel on the board.
- Disarm and saved-setup restore across a serial reset, still disarmed.
- Forget clears saved credentials for the next setup.
- Wizard UI inspected in the running Windows app: Companions navigation, COM5 selection, connection-mode step and Wi-Fi scan list including the user's requested SSID.

Automated checks cover firmware protocol/input safety, direct provisioning validation, storage round trips/corruption/failures, native USB identity correlation/pairing approval, bounded transfer chunks and digests. Svelte checks, 31 native companion-host tests, 3 serial transport tests and Windows native builds pass. Firmware builds with ESP-IDF 5.5.2 for Waveshare and generic headless targets.

The native smoke harness uses isolated host configuration and never prints passwords, pairing offers or tokens. Wi-Fi association/TLS pairing with the user's network still requires their password in the wizard. Physical finger input and visual verification of the latest live panel remain user tests. Arming currently requires USB; Wi-Fi sessions can continue after releasing USB while the board stays powered. Power-cycle restores connection settings but never automatically arms.

Windows mirror: C:/Users/ivang/AppData/Local/Temp/opensimgear-cockpit-windows-dev/workspace
