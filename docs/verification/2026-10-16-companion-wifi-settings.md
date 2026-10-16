# Saved connection settings, scanning and listener preference

- Firmware 2026.10.16.2 exposes saved name, mode, SSID and a password-present flag through USB status. Passwords and pairing tokens are never included. Cockpit prefills once per device selection and leaves subsequent user edits intact.
- Password inputs remain masked. An unchanged saved Wi-Fi password is reused on the board only when the SSID matches; a mismatched request fails before deleting the old setup.
- USB scans expose scanning/failure/timeout state and have a bounded 15-second host budget rather than the normal five-second status watchdog. Firmware active scan dwell is capped at 120 ms per channel. Results and scan failures update the UI.
- Listener desired-enabled state and address/port/discovery configuration are retained across startup delays and bind failures. Retry no longer consumes the saved configuration. The checkbox reflects saved intent; actual state remains Listening or Waiting to start. Stale polling responses cannot overwrite a toggle.
- Native firmware protocol/client tests pass, including saved password reuse and mismatched-SSID rejection. Serial suite: 14 passed. Windows companion-host suite: 32 passed, including restart with a blocked port, retained enabled preference, subsequent retry and persistent disable.
- Both physical board profiles cross-built with ESP-IDF 5.5.2 and were bundled in the Windows mirror; no physical firmware flash was performed for this release. Svelte checks passed.
