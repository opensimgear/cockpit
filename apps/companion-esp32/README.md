# ESP32 companion firmware foundation (#63)

This directory owns the portable C renderer and ESP-IDF firmware entry point. It starts the [ESP32 companion story](https://github.com/opensimgear/cockpit/issues/63) with tested rendering and a Wi-Fi/WSS display client. It supports serial Wi-Fi provisioning, pinned-CA host-approved pairing, assigned live panel downloads and typed feedback. The default display adapter is explicitly **headless**: it builds for both ESP32 and ESP32-S3 without guessing screen pins. Live momentary touch input activates automatically after the current panel is rendered, all touches are released and Cockpit acknowledges a fresh neutral baseline. Broader physical-device qualification remains open.

The shared partition layout requires at least 4 MB of flash. Headless builds default to 4 MB; board profiles supply their board-specific flash settings.

## Generic screen interface

The runtime depends only on rectangle and clipped text callbacks in core/companion.h. The included raster adapter turns them into synchronous RGB565 pixel spans with a 256-pixel (512-byte) scratch buffer. No full-screen framebuffer, heap allocation or PSRAM is required by this core. A driver can convert spans for RGB888, monochrome or a different byte order, or replace the drawing callbacks with an existing graphics library.

A physical screen still needs its correct bus/controller initialization, power/backlight, pins, dimensions, rotation and touch calibration. Screen dimensions alone cannot identify those details. The user's photograph now identifies the 7-inch board as Waveshare ESP32-S3-Touch-LCD-7 Rev 1.2. Its selectable RGB/GT911 adapter and exact flash/test steps are documented in [the board guide](boards/waveshare-lcd7/README.md). Physical qualification remains open; the smaller WROOM-32 is still unidentified.

Implement osg_board_init from boards/board.h in a board-specific C source. Return the actual dimensions, synchronous pixel writer and optional calibrated single-pointer touch poller. The application polls touch in a separate task every 20 ms; successful polls report the current released/pressed state and display-coordinate position. Driver failure returns false. Keep driver calls bounded and synchronize any shared display/touch bus in the adapter. Return false on initialization failure. Display-only adapters leave touch NULL. The core must be used under the application state mutex; marshal touch/feedback events rather than racing its state. Drawing uses an immutable snapshot outside that mutex. cJSON hooks are process-wide and reserved for the single network task; board adapters must not call this shared parser from another task.

For an initialized esp_lcd panel, boards/esp_lcd_bridge.h supplies the common pixel writer. Its wait_complete callback must fence transfer completion before the scratch buffer is reused. SPI/I80 adapters normally wait for on_color_trans_done; RGB adapters must use the appropriate copy/frame synchronization for their driver. The adapter owns panel color order/endian setup and DMA-capable staging if the controller requires it. Do not let a timed-out driver retain the supplied scratch pointer.

Adapters may optionally provide a bounded present callback after a complete render; its failure cancels live input before the panel is considered ready. No callback runs under the network-state mutex.

Pass a board source with -DOSG_BOARD_SOURCE=/absolute/path/board.c. Add driver component dependencies to main/CMakeLists.txt as necessary. Headless builds provide compilation evidence, not physical-screen support. The API supports different screens; drivers and performance must be qualified per board.

## Panel subset and identities

The offline exporter and live decoder consume existing DesignerDocument schema 2 publications; offline bundles use bundleVersion 1. It uses the existing designer validator, asset validation and publication implementation, then embeds an immutable C representation with the **original outer publication hash**, uiId, page/widget IDs, control IDs, feedback output IDs and channel IDs. The live client independently verifies canonicalDocument bytes against the assigned outer publication hash before staging a panel. It uses Cockpit's existing endpoint and messages rather than a replacement protocol. The offline exporter remains available for embedded fallback panels.

Supported: up to four pages, 32 widgets and a 64 KiB input bundle; momentary buttons, indicators, text, numeric and linear bar gauges (plus image widgets in live panels); normalized bounds, safe insets and RGB565 styles. Layout preserves each page's aspect ratio and letterboxes within the full physical display. Render and hit-test use the same geometry. Target export rejects touch targets smaller than 44 pixels; the runtime also refuses undersized buttons if the display is changed later.

Unsupported widgets (including toggle/pulse/repeat buttons, selectors, hats, encoders, axes and icons), groups, images in the offline C exporter and non-supported glyphs produce explicit export errors. Live image widgets are supported by firmware 2026.10.19.7 and newer. Initial text supports letters/digits, space and . , : ; - _ + = | / % ! ( ) ?. The original compact diagnostic font draws lowercase using uppercase glyph shapes; the stored labels and publication identities retain their original case. Text clips to its widget. A board may supply a richer text renderer later.

Typed local samples validate their channel, value kind, finite range, provenance and text budget before atomic assignment to every alias. Feedback shows fresh/stale/unknown/disconnected, retains provenance and includes source age; samples become stale at 1000 ms. The network decoder validates authenticated session/generation/ACK and feedback ordering, types, ranges, units and source provenance before atomically applying samples. Repeated feedback sequence numbers do not refresh sample timestamps.

Live input supports ordered momentary button edges through a bounded queue. Each connection starts with input inactive, then automatically activates after the rendered panel, released touch and connection are healthy. The firmware sends every button's neutral value in one session-bound baseline and waits for Cockpit's matching source-accepted ACK before enabling touch. This ACK describes host source admission; it does not claim native controller submission or game confirmation. Existing Cockpit mapping/backend gates still apply.

A touch must start inside a button; dragging into one cannot press it. Dragging out, changing page, touch/flush failure or a missed health lease cancels the session. Held buttons stay pressed while the touch and host leases remain healthy. Release edges preserve order, including press/release pairs within one input batch. Axes, hats, toggles, multitouch and gestures remain unsupported.

The queue holds 64 edges, sends at most 16 per batch and retains each reservation until a matching ACK. Pending edges, including in-flight edges, expire at 250 ms. Touch polls and active renders have a 600 ms lease; authenticated host ACKs have a 1000 ms lease, below Cockpit's 1500 ms source lease. Heartbeats run every 250 ms and input batches are spaced by at least 10 ms. Overflow, stale work, ambiguous baseline/input ACKs and counter exhaustion disarm locally, clear pending work and close the connection. No input is replayed after reconnect. Host socket loss/lease expiry neutralizes only this companion source. These are software limits, not measurements of physical Wi-Fi/display timing.

## Build and flash

This workstation has ESP-IDF v5.5.2 and its compiler/Python tools installed in WSL Ubuntu under the repository's ignored .research directory. From the repository root in PowerShell, activate it for one command with:

    powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\apps\companion-esp32\tools\idf.ps1 --version
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\apps\companion-esp32\tools\idf.ps1 -B build-waveshare-lcd7 -DIDF_TARGET=esp32s3 -DSDKCONFIG=build-waveshare-lcd7/sdkconfig -DOSG_BOARD_PROFILE=waveshare-lcd7-s3 build

The launcher resolves this checkout's WSL distribution and activates the SDK without changing PATH, shell profiles or global execution policy. Linux users can run bash apps/companion-esp32/tools/idf.sh with the same arguments. On another workstation, install the pinned SDK/tools first and set OSG_ESP_IDF_PATH inside WSL/Linux if it is outside the cached location; an existing IDF_PATH/IDF_TOOLS_PATH is also supported. These launchers do not download or install tools.

This is a WSL toolchain, not a native Windows installation. Flash/monitor/provision commands running in WSL need an actual Linux serial device, normally exposed through USB passthrough; Windows COM5 is not a WSL device path. Alternatively use a native Windows SDK and its COM port. No USB device was flashed as part of setup.

Pinned SDK: **ESP-IDF v5.5.2**, commit 30aaf64524299d3bde422ca9a2848090d1bc5d0f. Use the official SDK installation/export workflow. The firmware rejects another IDF version at compile time. No third-party graphics dependencies are downloaded by the firmware project. Pinned cJSON source and its MIT notice are retained under third_party; native tests and firmware compile the same parser.

From the repository root, Node 24+ can export a compatible designer bundle:

    node apps/companion-esp32/tools/export-panel.mjs panel.bundle.json /path/to/generated/panel.h 800 480

The destination directory must exist. A committed deterministic test/panel.h is regenerated from test/panel.bundle.json and checked by the exporter test.

From apps/companion-esp32 with IDF exported:

    idf.py -B build-esp32 -DIDF_TARGET=esp32 -DSDKCONFIG=build-esp32/sdkconfig build
    idf.py -B build-esp32s3 -DIDF_TARGET=esp32s3 -DSDKCONFIG=build-esp32s3/sdkconfig build

For the identified Waveshare 7-inch S3 board, use a fresh build directory and -DOSG_BOARD_PROFILE=waveshare-lcd7-s3; see [its flash guide](boards/waveshare-lcd7/README.md). This enables the real display/touch adapter and required PSRAM configuration. Other builds still default to headless.

Headless ESP32 and ESP32-S3 builds select 4 MiB flash to fit the shared application
and image partitions. Use a device with at least that capacity. Display board
profiles retain their own flash settings (Sunton 4 MiB, Waveshare 8 MiB).

Use -DOSG_PANEL_INCLUDE=/absolute/path/to/generated to embed another exported panel. To use a real screen, supply -DOSG_BOARD_SOURCE=/absolute/path/to/board.c and its matching dimensions/driver. Width/height menuconfig values apply to the headless adapter only. Use separate build/config paths per MCU.

Flash with the matching build's idf.py -B build-esp32s3 -p PORT flash monitor (or build-esp32). No flash or eFuse operation is performed by tests. Reflash a known-good image over serial for this milestone; Use the serial forget command to clear a saved installation. Automatic trust/config migrations and OTA remain future work. OTA is tracked in #64, broader board qualification in #65.

## Cockpit USB setup

Open **Companions → ESP32 companions**, select the USB serial device and click **Connect device**. Choose **Wi-Fi** to scan networks, select the SSID and enter its password; enable the companion listener in **Settings** first. Choose **Direct USB** to carry the companion protocol over USB without Wi-Fi or a network listener. Select a saved initial panel or assign one later in Cockpit, then click **Provision device**. Cockpit creates a one-time offer and approves only the installation identity it generated for this setup action. Pairing tokens and Wi-Fi passwords are never returned to the wizard.

Connection settings persist on the device; assignments and publications stay in Cockpit. Connect the device in the wizard again after restarting Cockpit. The generic UART transport uses 921600 baud after boot; ROM logs remain at 115200. Keep a direct device attached. Wi-Fi can run independently once setup is saved; power-loss restart waits for a healthy panel and released touch before activating input. Use **Reconnect panel**, **Pause touch input** and **Forget saved setup** in Cockpit without terminal commands. Touch input activates automatically after the panel has rendered, touch/display health checks pass and all touches are released. Every new session still performs the session-bound neutral-baseline handshake; failed baselines are not retried. Pause touch input stops the session until Reconnect panel is selected.

USB replies use ordered, SHA-256 checked chunks (2048 bytes decoded), bounded to the ESP32 reply budget. Native connection identities are freshly generated on each device link; serial link IDs never act as native authentication identities. Releasing USB or losing the port neutralizes only that direct session. No display or touch pins are part of the transport.

## Wi-Fi, pairing and live delivery

The generic UI needs no provisioning widgets, keyboard or camera. Use a trusted serial cable and the helper (Python 3 plus pyserial; the IDF Python environment includes it):

    python tools/provision.py COM5 --name "Cockpit screen" --interactive

Enter the Wi-Fi SSID/password, then paste a **fresh** pairing JSON offer from Cockpit's Companion panels view. Input containing passwords/pairing material is hidden and is never printed by the helper. The helper supplies the computer's current Unix time through the cable; this is the trusted time bootstrap for certificate validity, independently of the network.

The helper waits for a fresh device status before sending provisioning. On boards whose USB bridge resets the ESP32 when opening a port, keep --interactive enabled: use status, disarm, connect and page N at the Device command prompt without reopening the serial connection. quit stops the source and exits. One-shot commands below are intended for adapters that preserve the running device; repeated opens on the Waveshare reset the running session.

In Cockpit, enable the listener for Wi-Fi and complete the USB setup wizard. Assign exactly one controller instance to each ESP32; Cockpit controls its published panel. The ESP32 requests that assignment on connection and after Cockpit invalidates an obsolete session.

The listener's pairing expiresAtMs belongs to the **host's monotonic clock** and is opaque to the device. Cockpit enforces expiry; the device separately limits approval to 60 seconds. Initial provisioning supplies a current serial clock. On power-cycle restore, Wi-Fi reconnect uses SNTP (pool.ntp.org) to initialize time before opening TLS. SNTP is unauthenticated; it never replaces the pinned Cockpit identity or disables certificate date/chain/hostname validation. Networks blocking DNS or NTP require Cockpit to supply a fresh clock over USB before resuming.

The WSS endpoint must be explicit wss://HOST:PORT/companion, currently with a DNS name or IPv4 host. The device checks the offered CA's SPKI SHA-256 pin, CA constraints and validity before sending the secret/token. ESP-IDF then verifies the ordinary server certificate chain, time and endpoint hostname/IP against that CA. The requested WebSocket subprotocol is opensimgear-companion-v1. No redirects, plaintext fallback, arbitrary native commands or discovery-based trust are accepted.

Panel delivery uses the existing session-open publication and, when necessary, ordered transfer-start/chunk/end frames with an independent transport digest. Limits are 16 KiB per WebSocket message, 32 KiB assembled reply, 16 KiB canonical live document, 96 KiB outstanding cJSON allocations, four pages/32 widgets and an 8 KiB panel-string arena. Depth/node limits and duplicate-key/NUL/nonfinite validation apply. Two immutable RAM panel candidates allow validation before activation; a candidate currently being rendered is not overwritten. Failed updates retain the previous visible panel, explicitly disconnected and disarmed. Image widgets request device-sized RGB565+alpha pixels through authenticated image-chunk messages. Images render behind controls and use the full widget bounds. Cockpit resizes source assets before publication and resamples each device transfer to its exact rendered pixel size.

The 0x1e0000-byte images partition holds a shared persistent image cache. Entries use the source asset SHA-256, exact rendered width/height and pixel format version, so different panels and widgets reuse identical pixels. Changing a title or control layout does not invalidate unchanged images. Each entry uses a 4 KiB metadata sector and sector-rounded pixel data; the bounded index holds up to 128 entries. Least recently used unpinned entries are evicted when a new allocation needs space. The displayed and incoming panels pin their assets, and all incoming cache hits are pinned before any missing image is allocated. Cache pressure or fragmentation can reject a switch when protected assets leave insufficient contiguous space; active pixels are never relocated or erased to make room.

Headers and pixel CRCs are validated before reuse, and an entry is committed only after its entire download passes flash readback. Incomplete or corrupted entries become misses. Persistent recency uses small append records; cache hits write no pixel data, and only a full metadata journal requires metadata-sector renewal. The renderer reads mapped flash tiles without allocating a full-screen framebuffer, including on the Sunton without PSRAM. The per-panel raster limit remains 1200 KiB. The linked LVGL renderer and portable fonts use a 2 MiB application partition; the image cache starts at 0x210000. Flash the matching partition table when upgrading from the earlier foundation firmware; cached pixels are disposable and may need downloading again. NVS offsets are unchanged. Document/session state is still acquired from the host after reconnect; cached images never restore input authority or skip the neutral-baseline handshake.

Companions report an identity-bound panel operation with an attempt/session, phase, completed/total raster bytes, cache hits/misses and structured failure code/retry policy. USB status probes and authenticated image/heartbeat messages carry progress through opening, cache lookup, download, rendering and waiting for healthy released touch. Ready is reported only after the fresh neutral baseline succeeds. Cockpit uses validated progress to extend its activation wait (30-second progress idle limit, five-minute hard limit; legacy firmware retains its 60-second bound), fails promptly on terminal errors or a lost target session, and keeps acquired outputs neutral while waiting. USB stop/reconnect commands can interrupt loading at image-chunk boundaries. Companion tiles show progress or failures in their existing message hover, with full operation data available in debug status.

A dedicated network worker validates session IDs, generation, publication identity and counters before admitting ACKs or feedback. Current feedback snapshot/delta envelopes also validate binding generation, output bindings, types, ranges, units, source status and provenance. Invalid envelopes stop the session. The renderer uses immutable snapshots so drawing and DMA waits never hold the network-state mutex. A stalled render or touch poll cannot keep a live source healthy indefinitely.

After provisioning, touch activates automatically when the current panel has painted successfully, the touch driver reports released touch and fresh host acknowledgements are available. The automatic activation uses the current session, generation, publication and arm epoch, and sends a confirmed neutral baseline before admitting input. Headless/display-only adapters cannot activate touch. A held finger delays activation until release; touch during the baseline or an ambiguous baseline acknowledgement closes the connection without replaying it.

**Pause touch input** stops the session. **Reconnect panel** starts a fresh session and automatically repeats health, release and baseline checks. A panel assignment change also repeats these checks. Faults and revocation still require reconnecting; there is no automatic retry of a failed baseline. Legacy developer arm commands remain supported but are unnecessary for normal operation.

Connection settings and the approved installation token persist in NVS by default. Saved records include connection mode, Wi-Fi credentials and Cockpit trust/identity; they exclude the temporary pairing offer, wall clock, panel selection, publication contents and arming state. Cockpit owns panel assignment, which is requested on connection. Storage schema 2 rejects incompatible or malformed records without erasing unrelated NVS data. Ordinary boards use unencrypted NVS, so physical flash access can expose credentials. Firmware never burns eFuses or enables flash encryption automatically.

After restart the device restores its connection settings and activates touch only after a fresh connection, healthy rendered panel, released touch and confirmed neutral baseline. Use Forget to erase the installation. Developer tooling supports --no-persist for temporary sessions and --command resume for supplying a fresh trusted clock.


Do not put pairing JSON or Wi-Fi passwords in command arguments, logs, UI exports or source control. A copied offer file contains a short-lived secret. Connection/diagnostic output contains fixed status tokens rather than raw host errors or credential fields.

## Verification

From the repository root:

    node --experimental-test-coverage --test-coverage-include=apps/companion-esp32/tools/export-panel.mjs --test-coverage-lines=71 --test-coverage-branches=71 --test-coverage-functions=71 --test apps/companion-esp32/test/export.test.mjs
    python3 apps/companion-esp32/test/run.py
    python3 apps/companion-esp32/test/network.py
    python3 apps/companion-esp32/test/waveshare.py
    python3 apps/companion-esp32/test/serial.py
    python3 -m unittest discover -s apps/companion-esp32/test -p provision_test.py -v
    cargo test --locked --manifest-path crates/companion-service/Cargo.toml --test esp32_display

The rendering native harness needs GCC/gcov and Python 3, compiles with address/undefined-behavior sanitizers and gates each portable C module at 71% lines, branches and functions. It renders 800×480, 320×240 and 480×800, checks clipped pixels and creates .research/esp32-native/preview.png plus coverage.json. Inputs and feedback in that image are local fixtures, not game confirmation. The exporter tests compare the shared publication hash and committed generated header and exercise unsupported documents/resources/capabilities.

CI runs these checks, the portable network suite, the real host Hub compatibility test and pinned-IDF network-enabled ESP32/ESP32-S3 cross-builds with a headless display. Native adapter, physical LAN, durable panel cache/power-loss behavior, touch ergonomics and memory/performance on physical boards remain open #63 criteria.

Framework references: [ESP-IDF LCD abstraction and driver operations](https://docs.espressif.com/projects/esp-idf/en/v5.5.2/esp32s3/api-reference/peripherals/lcd/index.html), [SPI LCD setup](https://docs.espressif.com/projects/esp-idf/en/v5.5.2/esp32/api-reference/peripherals/lcd/spi_lcd.html).

The network native runner additionally needs OpenSSL development headers/libcrypto (Ubuntu libssl-dev). Its C client tests inject Wi-Fi/TLS/storage boundaries; they are not real radio or TLS-device qualification. The Rust integration test exercises the actual Cockpit Hub with the same publication fixture and proves that feedback is available without submitting input, then separately checks neutral-baseline admission, ordered live edges, isolated lease expiry, socket loss and fresh-session requirements. SDK TLS/Wi-Fi adapters are cross-compiled for both MCUs; their physical trust/storage behavior remains unqualified.

Detailed local evidence: [live-input checkpoint](../../docs/verification/2026-10-16-companion-esp32-live-input.md) and [earlier network checkpoint](../../docs/verification/2026-10-16-companion-esp32-network.md).

## Sunton 3.5-inch screen

The ESP32-3248S035R resistive-touch board has a dedicated [display/touch profile](boards/sunton-3248s035r/README.md) and Cockpit firmware installation option. It uses the original ESP32 target, not ESP32-S3.

Firmware 2026.10.20.1 adds the shared flash image cache and structured panel progress/failure feedback. USB and Wi-Fi failures distinguish timeouts, malformed replies and connection errors. Long messages wrap beneath the centered status icon and are also reported to Cockpit.

Firmware 2026.10.20.2 preserves partial USB records across idle polling and protocol requests. A status probe interrupted by a heartbeat no longer makes the heartbeat parse a truncated JSON record and stop the panel.

Firmware 2026.10.21.1 keeps an authenticated, unassigned screen on **No panel assigned** while it checks for assignments. A new panel still opens automatically, and a failed connection or authentication still displays its failure.

Engine 2026.10.22.1 adds common schema-2/schema-3 source admission and LVGL panel
rendering. Candidates are prepared in fenced immutable banks and checked by the
display task before adoption. Failed replacements keep the previous panel
disarmed with a diagnostic banner. A changed page reconnects safely when needed
and is preserved by stable page ID for the same assigned instance/UI; every
activation still requires a completed draw, released healthy touch and the
existing neutral baseline. Cockpit capability negotiation/assignment enablement
for portable widgets remains in feature #112; see
[runtime source details](render/WIDGET_SOURCE.md). Matching partition tables are
required for the larger application partition. Physical qualification is pending.
