# Companion tiles and guided pairing

Companions is a grid of remembered installations, showing connected/offline state, saved device name, type, assigned panel and removal. Panels remains a separate section below it. Pair opens a full section with browser/mobile/ESP32 selection. Device records and credential hashes are persisted in the host's protected store; credentials on the native companion stay in its protected store. Removal revokes credentials and releases owned sources. Old records without type metadata remain visible as generic companions.

Desktop pairing generates an eight-character random code, valid for sixty seconds. Native clients choose a discovered Cockpit host or enter its displayed WSS address. SPAKE2 authenticates an encrypted CA-bearing pairing offer; credentials are sent only after reconnecting with the authenticated CA pin. Code attempts are bounded to five, new offers replace old offers, cancellation removes unconsumed offers, and successful pairing consumes the secret. QR pairing carries the ordinary high-entropy, pinned pairing offer. Both flows approve only possession of the selected one-time offer.

A native credential-only presence connection keeps tiles current without opening an input source or renewing an input lease. It fetches current assignments and retries after network loss. Pairing success in Cockpit waits for an authenticated connection (and board-persisted setup for ESP32).

Mobile uses a camera QR scanner when BarcodeDetector is available, with code entry as fallback. Camera access remains permission-controlled by the OS. Android Keystore and Apple Keychain storage adapters remain the native persistence boundary; pairing can save credentials independently of mobile input/lifecycle qualification. Physical Android/iOS camera and lifecycle testing remains necessary; this change does not claim qualified mobile game input.

ESP32 setup requires a USB device and a matching board firmware profile. The included profile is Waveshare ESP32-S3 Touch LCD 7 Rev 1.2, 800×480. Cockpit embeds its three firmware segments, uses the ROM bootloader via espflash, checks the chip, refuses secured/encrypted boards, verifies writes, resets and reconnects before provisioning. Existing compatible firmware can connect directly. Wi-Fi setup scans networks and asks for the password; USB mode needs no Wi-Fi. NVS saves connection setup/credentials, with panel assignments controlled by Cockpit. The reset action checks the selected USB identity, forgets board setup and revokes the selected old installation before pairing again.

Build staging:
    python3 scripts/package-companion-firmware.py apps/companion-esp32/build-waveshare-lcd7

Then build Cockpit. A build without staged firmware disables that installation action rather than flashing an unknown image. The board CI artifact includes the staged package. No device backup or NVS image is embedded.

Validation: 68 companion-service tests, four serial/flashing tests and five native companion-host tests passed. Both Svelte checks passed without errors or warnings. Historical Cockpit Windows native and frontend production builds passed; desktop companion build evidence is retired after the browser migration. The Windows UI was inspected for retained companion tiles, type selection and the bundled Waveshare USB form.

Coverage includes real local WSS short-code pairing, sealed restart persistence, authenticated presence and revocation; code mismatch/tamper/cancel/expiry/single-use/attempt limits; firmware layout rejection; existing service and USB transport tests; Svelte checks and Windows native builds. Hardware flashing and physical mobile camera checks are separate from these host tests.

Protocol reference: https://docs.rs/spake2/0.4.0/spake2/
