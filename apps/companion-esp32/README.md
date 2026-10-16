# ESP32 companion firmware foundation (#63)

This directory owns the portable C renderer and ESP-IDF firmware entry point. It starts the [ESP32 companion story](https://github.com/opensimgear/cockpit/issues/63) with tested rendering and a Wi-Fi/WSS display client. It supports serial Wi-Fi provisioning, pinned-CA host-approved pairing, assigned live panel downloads and typed feedback. The default display adapter is explicitly **headless**: it builds for both ESP32 and ESP32-S3 without guessing screen pins. Live momentary touch input requires an explicit serial arm command and an acknowledged neutral baseline for the current session. Connections always start disarmed. Physical-device qualification remains open.

## Generic screen interface

The runtime depends only on rectangle and clipped text callbacks in core/companion.h. The included raster adapter turns them into synchronous RGB565 pixel spans with a 256-pixel (512-byte) scratch buffer. No full-screen framebuffer, heap allocation or PSRAM is required by this core. A driver can convert spans for RGB888, monochrome or a different byte order, or replace the drawing callbacks with an existing graphics library.

A physical screen still needs its correct bus/controller initialization, power/backlight, pins, dimensions, rotation and touch calibration. Screen dimensions alone cannot identify those details. The user's 7-inch 800×480 S3 and smaller WROOM-32 display remain physically unqualified.

Implement osg_board_init from boards/board.h in a board-specific C source. Return the actual dimensions, synchronous pixel writer and optional calibrated single-pointer touch poller. The application polls touch in a separate task every 20 ms; successful polls report the current released/pressed state and display-coordinate position. Driver failure returns false. Keep driver calls bounded and synchronize any shared display/touch bus in the adapter. Return false on initialization failure. Display-only adapters leave touch NULL. The core must be used under the application state mutex; marshal touch/feedback events rather than racing its state. Drawing uses an immutable snapshot outside that mutex. cJSON hooks are process-wide and reserved for the single network task; board adapters must not call this shared parser from another task.

For an initialized esp_lcd panel, boards/esp_lcd_bridge.h supplies the common pixel writer. Its wait_complete callback must fence transfer completion before the scratch buffer is reused. SPI/I80 adapters normally wait for on_color_trans_done; RGB adapters must use the appropriate copy/frame synchronization for their driver. The adapter owns panel color order/endian setup and DMA-capable staging if the controller requires it. Do not let a timed-out driver retain the supplied scratch pointer.

Pass a board source with -DOSG_BOARD_SOURCE=/absolute/path/board.c. Add driver component dependencies to main/CMakeLists.txt as necessary. Headless builds provide compilation evidence, not physical-screen support. The API supports different screens; drivers and performance must be qualified per board.

## Panel subset and identities

The offline exporter and live decoder consume existing DesignerDocument schema 2 publications; offline bundles use bundleVersion 1. It uses the existing designer validator, asset validation and publication implementation, then embeds an immutable C representation with the **original outer publication hash**, uiId, page/widget IDs, control IDs, feedback output IDs and channel IDs. The live client independently verifies canonicalDocument bytes against the assigned outer publication hash before staging a panel. It uses Cockpit's existing endpoint and messages rather than a replacement protocol. The offline exporter remains available for embedded fallback panels.

Supported: up to four pages, 32 widgets and a 64 KiB input bundle; momentary buttons, indicators, text, numeric and linear bar gauges; normalized bounds, safe insets and RGB565 styles. Layout preserves each page's aspect ratio and letterboxes within a 24-pixel status area and eight-pixel bottom margin. Render and hit-test use the same geometry. Target export rejects touch targets smaller than 44 pixels; the runtime also refuses undersized buttons if the display is changed later.

Unsupported widgets (including toggle/pulse/repeat buttons, selectors, hats, encoders, axes and icons), groups, images and non-supported glyphs produce explicit export errors. Initial text supports letters/digits, space and . , : ; - _ + = | / % ! ( ) ?. The original compact diagnostic font draws lowercase using uppercase glyph shapes; the stored labels and publication identities retain their original case. Text clips to its widget. A board may supply a richer text renderer later.

Typed local samples validate their channel, value kind, finite range, provenance and text budget before atomic assignment to every alias. Feedback shows fresh/stale/unknown/disconnected, retains provenance and includes source age; samples become stale at 1000 ms. The network decoder validates authenticated session/generation/ACK and feedback ordering, types, ranges, units and source provenance before atomically applying samples. Repeated feedback sequence numbers do not refresh sample timestamps.

Live input supports ordered momentary button edges through a bounded queue. Connecting, receiving a panel or reconnecting never arms controls. An operator must release touch and explicitly arm the current session; the firmware sends every button's neutral value in a single baseline and waits for Cockpit's matching source-accepted ACK before enabling touch. This ACK describes host source admission; it does not claim native controller submission or game confirmation. Existing Cockpit mapping/backend gates still apply.

A touch must start inside a button; dragging into one cannot press it. Dragging out, changing page, touch/flush failure or a missed health lease cancels the session. Held buttons stay pressed while the touch and host leases remain healthy. Release edges preserve order, including press/release pairs within one input batch. Axes, hats, toggles, multitouch and gestures remain unsupported.

The queue holds 64 edges, sends at most 16 per batch and retains each reservation until a matching ACK. Pending edges, including in-flight edges, expire at 250 ms. Touch polls and active renders have a 600 ms lease; authenticated host ACKs have a 1000 ms lease, below Cockpit's 1500 ms source lease. Heartbeats run every 250 ms and input batches are spaced by at least 10 ms. Overflow, stale work, ambiguous baseline/input ACKs and counter exhaustion disarm locally, clear pending work and close the connection. No input is replayed after reconnect. Host socket loss/lease expiry neutralizes only this companion source. These are software limits, not measurements of physical Wi-Fi/display timing.

## Build and flash

Pinned SDK: **ESP-IDF v5.5.2**, commit 30aaf64524299d3bde422ca9a2848090d1bc5d0f. Use the official SDK installation/export workflow. The firmware rejects another IDF version at compile time. No third-party graphics dependencies are downloaded by the firmware project. Pinned cJSON source and its MIT notice are retained under third_party; native tests and firmware compile the same parser.

From the repository root, Node 24+ can export a compatible designer bundle:

    node apps/companion-esp32/tools/export-panel.mjs panel.bundle.json /path/to/generated/panel.h 800 480

The destination directory must exist. A committed deterministic test/panel.h is regenerated from test/panel.bundle.json and checked by the exporter test.

From apps/companion-esp32 with IDF exported:

    idf.py -B build-esp32 -DIDF_TARGET=esp32 -DSDKCONFIG=build-esp32/sdkconfig build
    idf.py -B build-esp32s3 -DIDF_TARGET=esp32s3 -DSDKCONFIG=build-esp32s3/sdkconfig build

Use -DOSG_PANEL_INCLUDE=/absolute/path/to/generated to embed another exported panel. To use a real screen, supply -DOSG_BOARD_SOURCE=/absolute/path/to/board.c and its matching dimensions/driver. Width/height menuconfig values apply to the headless adapter only. Use separate build/config paths per MCU.

Flash with the matching build's idf.py -B build-esp32s3 -p PORT flash monitor (or build-esp32). No flash or eFuse operation is performed by tests. Reflash a known-good image over serial for this milestone; Use the serial forget command to clear a protected installation. Automatic trust/config migrations and OTA remain future work. OTA is tracked in #64, broader board qualification in #65.

## Wi-Fi, pairing and live delivery

The generic UI needs no provisioning widgets, keyboard or camera. Use a trusted serial cable and the helper (Python 3 plus pyserial; the IDF Python environment includes it):

    python tools/provision.py COM5 --name "Cockpit screen"

Enter the Wi-Fi SSID/password, then paste a **fresh** pairing JSON offer from Cockpit's Companion panels view. Input containing passwords/pairing material is hidden and is never printed by the helper. The helper supplies the computer's current Unix time through the cable; this is the trusted time bootstrap for certificate validity, independently of the network.

In Cockpit, explicitly enable the companion listener on a reachable LAN interface, make a compatible published revision available, approve the requesting screen and assign a controller instance. If there is exactly one assignment, it is selected automatically. With several assignments, choose one using --instance when provisioning or send:

    python tools/provision.py COM5 --command connect --instance INSTANCE_ID

The listener's pairing expiresAtMs belongs to the **host's monotonic clock** and is opaque to the device. Cockpit enforces expiry; the device separately limits its approval attempt to 60 seconds from provisioning. Certificate time comes only from the trusted serial clock. No unauthenticated NTP, time-check bypass or global/public CA fallback is used.

The WSS endpoint must be explicit wss://HOST:PORT/companion, currently with a DNS name or IPv4 host. The device checks the offered CA's SPKI SHA-256 pin, CA constraints and validity before sending the secret/token. ESP-IDF then verifies the ordinary server certificate chain, time and endpoint hostname/IP against that CA. The requested WebSocket subprotocol is opensimgear-companion-v1. No redirects, plaintext fallback, arbitrary native commands or discovery-based trust are accepted.

Panel delivery uses the existing session-open publication and, when necessary, ordered transfer-start/chunk/end frames with an independent transport digest. Limits are 16 KiB per WebSocket message, 32 KiB assembled reply, 16 KiB canonical live document, 96 KiB outstanding cJSON allocations, four pages/32 widgets and an 8 KiB panel-string arena. Depth/node limits and duplicate-key/NUL/nonfinite validation apply. Two fixed image slots allow validation before activation; a slot currently being rendered is not overwritten. Failed updates retain the previous visible panel, explicitly disconnected and disarmed. There is no durable downloaded-panel cache yet.

A dedicated network worker validates session IDs, generation, publication identity and counters before admitting ACKs or feedback. Current feedback snapshot/delta envelopes also validate binding generation, output bindings, types, ranges, units, source status and provenance. Invalid envelopes stop the session. The renderer uses immutable snapshots so drawing and DMA waits never hold the network-state mutex. A stalled render or touch poll cannot keep a live source healthy indefinitely.

After provisioning reports live-disarmed, configure the instance's controls and backend in Cockpit, release the screen and explicitly arm:

    python tools/provision.py COM5 --command status
    python tools/provision.py COM5 --command arm

The helper requests a fresh status using a unique request ID, then binds its one-shot arm command to sessionId, generation, publicationHash and armEpoch. The screen must have successfully painted the current panel and have fresh released touch and host acknowledgements. Headless/display-only adapters cannot arm. A touch during the baseline handshake cancels it; rejected arm tickets cannot be reused after release. Successful arming reports live-armed and the screen shows LIVE INPUT.

To stop input:

    python tools/provision.py COM5 --command disarm

Disarm stops local input before requesting host closure and reports disarmed-reconnect-required. Use --command connect and then a fresh --command arm to resume. Changing page while armed also closes the session. Faults and revocation require an explicit reconnect; neither reconnect nor a page/publication change restores arming.

Other commands:

    python tools/provision.py COM5 --command status
    python tools/provision.py COM5 --command page --page 1
    python tools/provision.py COM5 --command stop
    python tools/provision.py COM5 --command forget

Wi-Fi credentials and paired tokens stay in RAM by default, including Wi-Fi driver storage. After reboot, RAM-only installations need a fresh offer/re-pairing. --persist explicitly requires NVS encryption configured **and** flash encryption already enabled on the actual board; otherwise provisioning is rejected before secret exchange. No encryption setting or eFuse is enabled automatically. Secure-board provisioning/partition/key recovery must be qualified separately; do not enable it blindly on existing boards. Stored installations contain no pairing offer secret, and resume requires a fresh trusted serial clock:

    python tools/provision.py COM5 --command resume

Do not put pairing JSON or Wi-Fi passwords in command arguments, logs, UI exports or source control. A copied offer file contains a short-lived secret. Connection/diagnostic output contains fixed status tokens rather than raw host errors or credential fields.

## Verification

From the repository root:

    node --experimental-test-coverage --test-coverage-include=apps/companion-esp32/tools/export-panel.mjs --test-coverage-lines=71 --test-coverage-branches=71 --test-coverage-functions=71 --test apps/companion-esp32/test/export.test.mjs
    python3 apps/companion-esp32/test/run.py
    python3 apps/companion-esp32/test/network.py
    python3 -m unittest discover -s apps/companion-esp32/test -p provision_test.py -v
    cargo test --locked --manifest-path crates/companion-service/Cargo.toml --test esp32_display

The rendering native harness needs GCC/gcov and Python 3, compiles with address/undefined-behavior sanitizers and gates each portable C module at 71% lines, branches and functions. It renders 800×480, 320×240 and 480×800, checks clipped pixels and creates .research/esp32-native/preview.png plus coverage.json. Inputs and feedback in that image are local fixtures, not game confirmation. The exporter tests compare the shared publication hash and committed generated header and exercise unsupported documents/resources/capabilities.

CI runs these checks, the portable network suite, the real host Hub compatibility test and pinned-IDF network-enabled ESP32/ESP32-S3 cross-builds with a headless display. Native adapter, physical LAN, durable panel cache/power-loss behavior, touch ergonomics and memory/performance on physical boards remain open #63 criteria.

Framework references: [ESP-IDF LCD abstraction and driver operations](https://docs.espressif.com/projects/esp-idf/en/v5.5.2/esp32s3/api-reference/peripherals/lcd/index.html), [SPI LCD setup](https://docs.espressif.com/projects/esp-idf/en/v5.5.2/esp32/api-reference/peripherals/lcd/spi_lcd.html).

The network native runner additionally needs OpenSSL development headers/libcrypto (Ubuntu libssl-dev). Its C client tests inject Wi-Fi/TLS/storage boundaries; they are not real radio or TLS-device qualification. The Rust integration test exercises the actual Cockpit Hub with the same publication fixture and proves that feedback is available without submitting input, then separately checks neutral-baseline admission, ordered live edges, isolated lease expiry, socket loss and fresh-session requirements. SDK TLS/Wi-Fi adapters are cross-compiled for both MCUs; their physical trust/storage behavior remains unqualified.

Detailed local evidence: [live-input checkpoint](../../docs/verification/2026-10-16-companion-esp32-live-input.md) and [earlier network checkpoint](../../docs/verification/2026-10-16-companion-esp32-network.md).
