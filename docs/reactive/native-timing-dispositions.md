# Native timing dispositions

This inventory uses the timing ledger pinned to Cockpit `56b6d1ec`,
device-gateway `da8ab4b`, device-template `c545788`, and firmware-common
`64025cc`.
Each row records the timing owner, its trigger, and the remaining evidence.
Changes after that revision are identified by their delivery commit.
Run `node scripts/reactive/audit.mjs --check-dispositions` to verify that the
native and frontend tables together account for every entry in the pinned
ledger exactly once.

2026-11-05 peer I/O update: the companion server's 100 ms socket read poll
(not a separate site in this historical sleep/channel-wait ledger) is retired.
Each peer owns nonblocking Mio I/O, actual handshake/idle/probe/write deadlines
and a direct cancellation Waker. Native client read polls are also retired;
native pending pairing also uses one terminal approval subscription. See [client verification](../verification/2026-11-05-native-client-event-io.md).
See [verification](../verification/2026-11-05-companion-peer-io.md).

2026-11-05 feedback update: the active profile publisher's 34 ms maximum rate
is no longer an unconditional full-snapshot cadence. Channel changes, independent
freshness renewals and exact quality boundaries select bounded delta delivery.
This site is not a separate row in the historical inventory. See
[feedback verification](../verification/2026-11-05-feedback-publication.md).

2026-11-05 ESP32 pairing update: firmware's repeated 400 ms `pair-status`
requests are removed for Wi-Fi and direct USB. One terminal subscription owns
approval/error delivery; original expiry and link cancellation remain separate.
This firmware request cadence was not a row in the pinned native wait inventory.
Physical touch sampling, session heartbeats and render deadlines are unchanged.
See [ESP32 pairing verification](../verification/2026-11-05-esp32-pairing-events.md).

| Ledger site | Owner and trigger | Disposition and verification |
| --- | --- | --- |
| `src-tauri/src/activity/mod.rs:107` | Activity writer flush, explicit request waits for its own acknowledgment | Keep the bounded flush deadline. Committed-only Activity reads skip the flush. Activity writer and read-model tests cover journal publication. |
| `src-tauri/src/activity/writer.rs:20` | Activity writer, committed metadata groups, journal persistence, lifecycle reconciliation, and pruning | Aggregate 750 ms metadata acquisition removed. Four retained source epochs select passive captures; lifecycle batches and explicit commands retain direct wakes. Only outstanding failed persistence uses a retry deadline; hourly pruning is separate. Idle capture, scoped acknowledgements and actual remote disconnect publication are covered in [Activity verification](../verification/2026-11-05-activity-source-wakes.md). |
| `src-tauri/src/companion/host/detection.rs:109` | Test-only wait for an occupied listener port to become available | Keep the test bound. The fixture verifies saved listener recovery and shutdown. |
| `src-tauri/src/companion/host/firmware_observation.rs:61` | Test-only wait for the terminal firmware observation | Keep the test bound. The fixture verifies that late subscribers get the same terminal status. |
| `src-tauri/src/acr_setup.rs:652` | Assetto setup probe, wait for another telemetry sample | Removed in `8e4469d3`. Journal publication, cancellation and the existing deadline wake the probe. The native cancellation regression passed. Authorized live delivery remains in #199. |
| `src-tauri/src/dcs_bios/transport_client.rs:32` | DCS-BIOS reader, wait for committed source data or caller deadline | Keep the condition-variable wait. Transport tests cover delivery and retirement. |
| `src-tauri/src/dcs_physics/source.rs:133` | Ownship reader, wait for a committed capture before admission | Keep the source notification and caller deadline. Source tests cover initial delivery. |
| `src-tauri/src/dcs_physics/source.rs:248` | Ownship source, expire the original freshness lease | Keep the source deadline. Expiry tests cover withdrawal and recovery. |
| `src-tauri/src/game_activation_runtime.rs:64` | Global activation observer, scan the game process catalogue | Replaced in `ebf1aa78` by a native event owner and initial exact inventory. A catalogue without monitorable installations parks until a committed library update. An installed game with process monitoring support still uses the 500 ms discovery fallback when Windows denies event access. The authored start/exit fixture passed through that fallback. |
| `src-tauri/src/game_metadata_api.rs:171`, `src-tauri/src/games/integration/catalogues.rs:42`, `src-tauri/src/games/integration/provision.rs:208,236`, `src-tauri/src/games/integration/settings.rs:26`, `src-tauri/src/games/integration/setup_cache.rs:181`, `src-tauri/src/games/prepared_sources/mod.rs:142`, `src-tauri/src/games/storage/car_catalogue.rs:160` | Test-only barriers during metadata, setup or catalogue publication | Keep bounded test waits. These fixtures verify publication, cancellation and persistence without deadlocks. |
| `src-tauri/src/games/context_sdk_watch.rs:110,118` | Test-only waits for an SDK file notification or quiet interval | Keep test bounds. The file-watch fixture covers native wake and cache refresh. |
| `src-tauri/src/games/discovery/launch.rs:226` | Historical synchronous launch helper | The synchronous helper is exercised only by tests. Production launch uses `spawn_process` and `wait_profile_launch`, which wait on activation notifications and the startup deadline while the activation worker is live. The worker can use the 500 ms inventory fallback when event coverage is unavailable. With no live worker, launch uses a scoped 100 ms fallback until the startup deadline. External launch and handoff still need live qualification. |
| `src-tauri/src/games/metadata/network.rs:58` | Metadata worker, retry after a failed Wikidata lookup within its budget | Keep bounded backoff. Offline and budget tests cover the worker behavior. |
| `src-tauri/src/games/telemetry/windows_source/mod.rs:172` | Opt-in simulator test, wait for the next fresh sample | Test-only. Real publisher qualification remains in #199. |
| `src-tauri/src/games/telemetry/windows_source/shared.rs:168` | Assetto native capture owner, sample shared memory or retry after read failure | Keep the approved acquisition cadence. Reader tests cover order, mapping generation and lease-loss replacement. |
| `src-tauri/src/profile_session/game_publication.rs:131` | Returning game reader, wait for fresh baseline before admission | Replaced in `22eae05e` by a producer wake and the existing deadline when the source can notify. Sources without a registered wake retain the bounded fallback. |
| `src-tauri/src/xplane/owner.rs:210` | X-Plane owner, wait for a selected subscription reply | Keep the command deadline. Owner tests cover selection and cancellation. |
| `src-tauri/src/xplane/owner/worker.rs:218` | X-Plane source worker, next renewal or capture expiry deadline | Keep protocol and freshness deadlines. Native tests cover renewals and source expiry. |
| `src-tauri/src/xplane/owner/worker.rs:269` | X-Plane source worker, retry unsubscribing while draining | Keep bounded protocol cleanup. Owner tests cover loss and shutdown. |
| `src-tauri/src/xplane/process_events/bridge.rs:139`, `src-tauri/src/xplane/process_events/windows.rs:213` | Native process monitor, bounded helper or direct startup | Keep startup deadlines. Native tests cover cancellation and lost readiness. User-operated Windows consent remains in #199. |


## 2026-11-06 committed scan

The Cockpit scan at `c69befcb` found 265 timing matches across Cockpit,
device-gateway `e761e4d6`, device-template `c545788` and firmware-common
`64025cce`. The original 247-site ledger stays pinned for historical
dispositions. The following review covers files with a net increase in matches
by file and primitive. The complete current acquisition gate remains open.

| Added matches | Owner and trigger |
| --- | --- |
| Controller report-cost example | Authored pacing in the standalone measurement example. |
| Desktop audit capacity and game command worker | One desktop audit condition-variable wait for the caller's capture deadline. Other added matches in these files are inline test barriers and simulated delays. |
| Desktop focus lifetime and native focus | Wait for the previous scope owner to exit or for the new owner to start. Cleanup waits once for its completion after posting quit. |
| Desktop feeder serve and wait, mapping-host desktop output worker | Bounded feeder completion, pending-command notifications and exact deadlines, and output lease renewal. The feeder wait file also contains a test delay. |
| Gateway and process exit watch | Inline regression barriers and an authored test process exit delay. |
| Controller observations | One frame callback declaration and one render callback scheduled when a new sample arrives. |
## Native crate timing sites

| Ledger site | Owner and trigger | Disposition and verification |
| --- | --- | --- |
| `crates/controller-input/src/capture.rs:325` | Opt-in Linux and macOS qualification probe, 4 ms raw-device read and a separate 500 ms inventory check | Ordinary Cockpit startup does not enable this capture path. Keep the probe acquisition unaccepted until native report and topology events are qualified. |
| `crates/mapping-host/src/service.rs:123` | Mapping-host service worker, read input cursors and flush output every 2 ms | Use publication wakes and output deadlines if this path gains a Cockpit production caller. No current Cockpit constructor call found. Service tests cover the crate path. |
| `crates/mapping-host/src/companion_runs.rs:1190` | Dormant legacy mapping-host companion route worker | No current Cockpit caller: the gateway rejects retired mapping commands, including `mapping_companion_route`, before worker dispatch. Keep the crate fixture classification; the active profile host has its own committed-source/deadline scheduler. The retired-command native regression covers rejection without adapter acquisition. |
| `crates/desktop-output/src/worker.rs:431` | Desktop feeder worker, wait for command or the next lease check | Native commands and foreground/desktop/session notifications directly wake the worker. An owned high-resolution timer waits for the nearest live lease or release guard; no obligations means an indefinite event wait. The native 5 ms focus acquisition fallback is removed; explicit recording adapters retain test sampling. Immutable registration/epoch proofs fence queued jobs and validated scope receipts reconcile parent authority. See [verification](../verification/2026-11-05-desktop-helper-focus.md). |
| `crates/controller-input/src/visibility/launch.rs:199`, `crates/controller-input/src/visibility/launch_linux.rs:404`, `crates/virtual-controller/src/supervisor.rs:248`, `crates/virtual-controller/src/windows_jobs.rs:208`, `crates/virtual-controller/src/windows_refresh.rs:184,201`, `crates/desktop-output/src/transport.rs:154` | Child or helper lifecycle, bounded reap or cancellation waits | Use OS process waits where available and retain the shutdown bound. Windows helper cleanup and Linux child exit need platform qualification. |
| `crates/controller-input/src/visibility/runtime.rs:63` | Visibility preparation, wait for one bounded native reply | Keep the message wait and deadline. Visibility startup tests cover cancellation. |
| `crates/controller-input/src/visibility/runtime.rs:382,429` | Visibility verifier, wait for fresh capture and check external policy | Use capture observation wakes for freshness. Qualify native external policy notifications before removing the audit timer. |
| `crates/mapping-host/src/game.rs:381,488` | Mapping-host game lifecycle, bounded acquire or restore wait | Replace repeated state checks with completion wakes and keep deadlines. Existing game tests cover restore. |
| `crates/companion-serial/src/boot_wait.rs:35` | Firmware installer, retry bootloader reconnect after failure | Keep bounded retry, with cancellation able to interrupt it. Boot wait tests cover expiry. |
| `crates/companion-service/src/client_feedback.rs:41` | Feedback receiver, reconnect after a transport failure | Keep the 250 ms retry deadline; scoped cancellation wakes it directly. Native blocked-TLS cancellation passes against the running Cockpit; mobile app qualification remains open. |
| `crates/companion-service/src/client.rs:1038` | Pending pairing, check operator approval every 400 ms | Retired: one secret-bound subscription receives the terminal host result, with the original offer expiry. Approval races, rejection, cancellation, expiry, revocation and reader retirement are covered; mobile qualification remains open. |
| `crates/companion-service/src/server.rs:437` | Historical listener accept retry | Retired 2026-11-05: `server/listener.rs` waits on socket readiness, earlier Hub expiry, peer completion and Stop. Native TLS/WSS, autonomous expiry and actual Cockpit reload/Stop pass; see [verification](../verification/2026-11-05-companion-listener-readiness.md). |
| `crates/desktop-output/src/audit/collector.rs:454` | Output audit flush, wait for space in the bounded writer queue | Retired: capacity dequeue and writer termination directly wake pending barriers. The original absolute reply deadline remains. Saturation, timeout, queued reply loss, I/O failure, panic and single-footer tests pass; actual Cockpit checkpoints and private-worker footers are valid. See [verification](../verification/2026-11-05-desktop-audit-capacity.md). |
| `crates/companion-service/src/client.rs:1363` | mDNS receive, wait for a reply within the discovery deadline | Keep bounded blocking receive. Qualify actual discovery separately. |
| `crates/streams/src/journal.rs:273,303`, `crates/streams/src/native/receive.rs:74,99`, `crates/streams/src/wake.rs:76` | Shared stream receive and worker wake primitives | Keep event waits and caller deadlines. Stream conformance tests cover wake, loss and retirement. |
| `crates/controller-input/src/registry/inventory_observation/queue.rs:119`, `crates/controller-input/src/registry/observation/queue.rs:303` | Controller observers, wait for ready or queued publication | Keep condition-variable deadlines. Observer tests cover readiness and loss. |
| `crates/persistence/src/observation.rs:80,153` | Persistence journal, failed-write retry and flush reply | Keep bounded retry and acknowledgment waits. Persistence tests cover recovery and flush. |
| `crates/companion-service/src/client.rs:870`, `crates/logic/src/typed_worker.rs:94`, `crates/logic/src/worker.rs:80`, `crates/virtual-controller/src/supervisor.rs:144,229`, `crates/desktop-output/src/transport.rs:100`, `crates/desktop-output/src/worker.rs:474`, `crates/virtual-controller/src/worker.rs:113,163`, `crates/desktop-output/src/audit/collector.rs:432` | Native command and lifecycle owners, wait for replies or lease deadlines | Keep bounded channel waits. Verify actual driver receipt separately from native submission. |
| `crates/companion-serial/src/lib.rs:1267,1269`, `crates/logic/src/replay.rs:27`, `crates/persistence/src/observation.rs:253`, `crates/virtual-controller/src/inspection.rs:234` | Test-only completion bounds and deadline fault injection | Keep bounded fixture timing. These sites have no production caller. |
| `crates/controller-input/examples/inspect.rs:10`, `crates/desktop-output/examples/native_receiver.rs:361` | Example readers, pace demonstration output | Keep example behavior. These are outside the application. |
| `crates/persistence/src/path/journal.rs:7` | SQLite rollback journal path verifier, retry transient Windows metadata access denial | Keep the bounded 1, 2 and 4 ms metadata retry and revalidate the database path before each attempt. Journal path tests cover rejection and disappearance. |

## Gateway, USB and observation timing sites

| Ledger site | Owner and trigger | Disposition and verification |
| --- | --- | --- |
| `src-tauri/src/gateways.rs:1788,1789,1791,1808,1811,1817,1821,1824,1857,1862,1865,1887,1905` | Test-only profile-wake barriers and quiet windows | Keep fixture bounds. Tests cover notification order, expiry and shutdown. |
| `src-tauri/src/gateways.rs:2018,2141,2145,2187,2192,2205,2234,2241,3018` | Test-only delayed requests and preparation barriers | Keep fixture bounds. Tests cover Stop, late completion and superseding ownership. |
| `src-tauri/src/gateways.rs:2489,2499,2956,2967,3060,3080` | Test-only CAN and slow gateway fixtures | Keep fixture bounds. Tests cover route loss, lock isolation and interruption. |
| `src-tauri/src/gateways/workspace/worker.rs:85` | Workspace owner, wait until the next lifecycle deadline | Keep notification and deadline wakes. Workspace tests cover reconnect and retired claims. |
| `src-tauri/src/observations/commands.rs:114`, `src-tauri/src/observations/service_output.rs:128,135,148,162` | Test-only delivery and blocked writer barriers | Keep fixture bounds. Tests cover acknowledgments, overflow and isolation. |
| `src-tauri/src/profile_resources/host_observation.rs:115,116,119,120,122,143,145,149` | Test-only resource publication and quiet windows | Keep fixture bounds. Tests cover independent USB, controller and aggregate wakes. |
| `src-tauri/src/module_usb.rs:519` | USB configuration worker, bounded settle wait during reconciliation | Keep the hardware deadline and make shutdown wake it directly. Physical USB qualification remains open. |
| `src-tauri/src/module_usb/test_io.rs:22` | Test helper, wait for a recorded command | Keep the condition-variable deadline. USB fixtures verify exact command counts. |
| `src-tauri/src/pico.rs:373` | Pico service, wait for command or heartbeat | Disconnected legacy service waits for commands. Connected claim and inactivity deadlines are independent of the two-second protocol 1 snapshot. Snapshot acquisition and physical gateway qualification remain open. |
| `src-tauri/src/pico.rs:829,835,867,897` | Pico reconnect, initial delay and bounded retry | Keep retry deadlines and make cancellation interrupt the wait. Reconnect tests cover shutdown. |
| `src-tauri/src/pico.rs:1218,1791,1865,1875` | Test-only serial stalls, shutdown and queue checks | Keep fixture bounds. Tests cover delayed replies and trusted ownership. |
| `src-tauri/src/pico/workspace.rs:37`, `src-tauri/src/stm32/workspace.rs:112` | Gateway workspace claims, wait for an explicit renewal reply | Keep the request deadline and verify the exact claim survives renewal. |
| `src-tauri/src/stm32_panel_observations.rs:356`, `src-tauri/src/stm32_panel_serial.rs:244` | Test-only panel observation and serial delivery waits | Keep fixture bounds. Tests cover native events and metadata. |
| `src-tauri/src/stm32_panel_serial.rs:91` | Serial command owner, wait for an exact reply | Keep the channel deadline. Serial tests cover parallel events and replies. |
| `src-tauri/src/stm32.rs:1034,1040,1083,1123,1161,1206,1726,1764` | STM32 configuration owner, settle, apply, recover and read layout | Preserve hardware deadlines. Replace readiness checks with configuration and reconnect events. Physical recovery remains open. |
| `src-tauri/src/stm32.rs:1337` | Action discovery, retry a failed route metadata or state request | Replace the 50 ms wait with a route-change wake. Keep the exact route fence and retry deadline. |
| `src-tauri/src/stm32.rs:2357` | STM32 service, wait for commands, inventory events and local authority deadlines | CAN and Pico Debug inventory refresh from sequenced gateway events. Setup claim expiry and unowned connection inactivity keep separate deadlines. Pico Debug renews its input watchdog with a compact command on idle connections. Physical gateway acceptance remains in #199. |
| `src-tauri/src/stm32.rs:2932,2935` | Test-only serial stalls | Keep fixture delays. Tests cover cancellation and partial replies. |
| `src-tauri/src/system_inventory.rs:248,254,279,287` | Test-only publication and pinned-page barriers | Keep fixture bounds. Tests cover passive reads and isolation from blocked capture. |

## Activation, persistence and desktop timing sites

| Ledger site | Owner and trigger | Disposition and verification |
| --- | --- | --- |
| `src-tauri/src/desktop/progress/geometry.rs:101`, `src-tauri/src/desktop/progress/mod.rs:245` | Activation progress window, wait for the UI thread to apply geometry or show the window | Keep command acknowledgments with deadlines. Progress window tests cover placement and ownership. |
| `src-tauri/src/firmware_releases.rs:414,417` | Test-only pending firmware operation and concurrent status read | Keep fixture bounds. The test verifies independent metadata reads. |
| `src-tauri/src/hid_names.rs:116` | HID naming worker, retry after CDC re-enumeration | Use device arrival notifications with a bounded retry deadline. Verify unplug and replacement on hardware. |
| `src-tauri/src/module_console/actor.rs:177` | Console actor, wait for requested replies or deadline | Keep the condition-variable wait. Console tests cover reply isolation. |
| `src-tauri/src/persistence/backup/startup.rs:96` | Explicit restore restart, defer the restart callback | Keep the one-shot handoff and verify the restored application opens. |
| `src-tauri/src/persistence/backup/startup.rs:143` | Test-only wait for isolated boot callback | Keep the fixture bound. The test verifies restart dispatch. |
| `src-tauri/src/persistence/ui/window_writer.rs:27,41` | Window persistence writer, retry busy storage and wait for flush | Keep bounded retry and acknowledgment. Tests cover flush, coalescing and ownership conflicts. |
| `src-tauri/src/persistence/ui/window_writer.rs:62,63` | Test-only window-writer commit barriers | Keep fixture bounds. The test verifies pending geometry is fenced by a competing commit. |
| `src-tauri/src/profile_automatic.rs:841` | Automatic activation worker, check vehicle, peripheral and process context | Replaced by committed context and control wakes, registry unlock after contention, and the next retry deadline. The final 500 ms fallback was removed in `bde8bbc2`. |
| `src-tauri/src/profile_game_launch.rs:320` | Explicit profile launch, wait for its exact process | Startup waits for the committed observer and launch control notifications, or the absolute deadline. The unavailable-observer fallback retains its 100 ms bound. Every candidate still receives direct exact identity verification. |
| `src-tauri/src/profile_publication.rs:884` | Temporary CAN publication actor, wait for Start or Stop | Start and Stop wake the actor directly. The exact authority fence still rejects a stopped actor. |
| `src-tauri/src/profile_publication.rs:1116` | Temporary CAN publication actor, check native owner and restoration progress | The gateway emits sequenced ownership and delivery invalidations. Cockpit schedules readback on the stream, Start/Stop and actual lease and cleanup deadlines. The old 20 ms State/Inspect loop is removed. Physical gateway acceptance remains in #199. |
| `src-tauri/src/profile_samples/registration.rs:59` | Profile preview subscription, wait for initial readiness | Keep the condition-variable deadline. Registration tests cover capture and cancellation. |
| `src-tauri/src/profile_session_companion_host.rs:404` | Profile activation worker, wait for a companion source baseline | Companion presence, panel operations and resource commits wake readiness. The bounded wait still services neutral leases and checks cancellation between events. |
| `src-tauri/src/profile_session_launch_host.rs:302` | Launched game reader, wait for a fresh provider baseline | The launch waiter uses producer publication and the existing admission deadline when the source can notify. Sources without a registered wake retain the bounded fallback. |
| `src-tauri/src/profile_session_panels_inputs.rs:242` | Panel baseline test helper | The helper is test-only. Production admission uses its native source and activation wake paths. |
| `src-tauri/src/profile_session_panels_inputs/can.rs:41,45` | Test-only CAN reader, wait for retry or bounded read capacity | Production CAN uses the native panel journal. Keep the fixture bounds. |
| `src-tauri/src/profile_session_panels_inputs/test_support.rs:68,355` | Test-only CAN or USB barrier and eventual assertion | Keep fixture bounds. Tests cover source publication and cancellation. |
| `src-tauri/src/profile_session/activation_heartbeat.rs:28` | Activation lease worker, wait for Stop or check next renewal deadline | Preserve renewal and Stop. Replace the 10 ms slices with the next actual deadline when cancellation is fully signaled. |
| `src-tauri/src/profile_system_notification.rs:326` | Windows notification worker, await dismissal callback after a missing update | Use the dismissal event with the existing 50 ms bound. Verify activation notification dismissal and late callbacks. |
| `src-tauri/src/window_persistence.rs:12` | Window state writer, debounce user resize or move | Keep the cancellable one-shot save. Tests cover coalescing and shutdown. |
| `src-tauri/src/window_persistence.rs:50,54,57,65` | Test-only window save acknowledgments and quiet windows | Keep fixture bounds. Tests cover resize bursts and idle shutdown. |

## Timing after the pinned inventory

| Boundary | Clock and owner |
| --- | --- |
| Physical controller, USB and CAN input | The device or firmware collector owns report cadence and capture time. Native blocking readers and bounded journals deliver those reports to routing, UI and MCP. CAN firmware without panel streaming exposes output capabilities only. |
| Simulator input | DCS and X-Plane publishers own report cadence. Native receiver leases expire from original captures. The authorized Assetto shared-memory reader owns the host sampling cadence and read-error backoff. Downstream consumers wake on committed capture. |
| Game process discovery | The Windows inventory worker owns process-event continuity. Where event access is denied it still owns the 500 ms discovery fallback. The process fallback remains an open acquisition disposition. |
| Profile reconciliation | The selector owns vehicle and peripheral backoff. It parks between source changes, registry release after contention, and the next retry deadline. The returning-game reader waits for the native capture or its admission deadline where the source offers a wake. |
| Output safety and authored behavior | The profile runtime schedules reducer and script deadlines and virtual-output renewals separately. Companion connection owners hold the panel lease and watchdog. Freshness expiry and timed actions can wake their owner while input is quiet. |
| Qualified controller and game profile routing | Windows native HID and internal sources with retained report and resource-commit watches opt out of 5 ms adapter service. Supported Assetto, X-Plane and DCS runs with retained telemetry watches use capture notifications and exact proof deadlines. X-Plane publishes component expiry, and Assetto schedules its decoder deadline on the host. DCS requires BIOS and any selected ownship watch plus independent catalogue proof. Missing watches, deferred inventory, BIOS retry and recovery keep bounded service. Optional vJoy reports drain immediately while queued. A terminal feeder reply or closed reply pipe wakes the native profile host. Quiet output retains its 50 ms renewal deadline and existing 250 ms lease. |
| Idle profile coordinator | With no applied owner or staged launch, the coordinator parks without a service timer. Apply, Stop and explicit launch transitions notify after ownership changes. |
| Held game launch | The coordinator uses the exact 90-second hold expiry alongside bounded hardware health service. Arming, finishing and cancelling a hold notify the coordinator so its wait reflects the new ownership and deadline. |
| Launched process confirmation | Startup and confirmation share the launch notification. Startup checks exact native identity after a wake. Confirmation uses committed native process proof, admission changes, hold status, and its own three-second deadline. |
| Desktop authored timing | Tap transitions, repeats, active pointer cadence and isolated desktop reducer deadlines reach the host scheduler. Parent/helper scope evidence comes from WinEvent/WTS notifications with retained loss epochs and direct admission revocation. Qualified native HID/internal desktop profiles now use retained wakes and deadlines instead of host 5 ms service. The output owner requests fresh parent authorization at the actual 100 ms renewal boundary; stale authorization never renews alone. The recent-application chooser shares the event owner instead of acquiring foreground on every profile iteration. Other adapter combinations, controlled focus/child-loss and timing/load qualification remain open. [Verification](../verification/2026-11-05-desktop-host-wakes.md). |
| Presentation | Widget playback schedules dirty frames while active. The panel controller owns the next action or feedback-expiry deadline. Dialog countdowns, edit debounces and notifications stay local to presentation. |
