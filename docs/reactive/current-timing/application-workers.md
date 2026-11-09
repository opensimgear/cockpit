# Application worker timing sites

| Site | Category | Owner and trigger |
| --- | --- | --- |
| `src-tauri/src/activity/mod.rs:136` | Operation deadline | Explicit flush waits up to 15 seconds for its own writer acknowledgement. |
| `src-tauri/src/activity/writer.rs:87` | Source wake and deadlines | Committed producer groups wake Activity. Hourly pruning and an outstanding persistence retry supply its timed wakes. |
| `src-tauri/src/companion/host/detection.rs:77` | Source wake and retry | Native port or hub-record changes wake listener and USB detection. An outstanding listener bind or exact USB reconnect can request a retry. |
| `src-tauri/src/companion/host/detection.rs:109` | Fixture | Wait for an occupied port to become available in the listener recovery test. |
| `src-tauri/src/companion/host/firmware_observation.rs:70` | Fixture | Bound a wait for the terminal firmware status. |
| `src-tauri/src/companion/host/presence_observation.rs:46` | Source wake | Wait for committed companion presence or shutdown. |
| `src-tauri/src/desktop/progress/geometry.rs:101` | Operation deadline | Wait up to two seconds for native popup geometry work on the window thread. |
| `src-tauri/src/desktop/progress/mod.rs:245` | Operation deadline | Wait up to two seconds for the current popup operation on the window thread. |
| `src-tauri/src/firmware_releases.rs:440,443` | Fixture | Bound start and status barriers while testing firmware release work. |
| `src-tauri/src/persistence/backup/startup.rs:96` | Operation deadline | Give an explicit restore-restart acknowledgement 500 ms before calling the native restart callback. |
| `src-tauri/src/persistence/backup/startup.rs:143` | Fixture | Bound the isolated restart callback test's wait for completion. |
| `src-tauri/src/persistence/ui/window_writer.rs:71` | Source wake | Park the persistence writer until queued geometry or shutdown. |
| `src-tauri/src/persistence/ui/window_writer.rs:90` | Persistence retry | Retry a busy save after 250 ms if there is no newer queued geometry and the bounded retry count permits it. |
| `src-tauri/src/persistence/ui/window_writer.rs:104` | Operation deadline | Wait for an explicit flush to complete within its budget. |
| `src-tauri/src/persistence/ui/window_writer.rs:147,148` | Fixture | Bound the geometry coalescing test's commit barrier. |
| `src-tauri/src/profile_activation_progress/observation.rs:72` | Source wake and expiry | Committed activation and companion work wake the progress projector. The nearest presentation or retained operation expiry also wakes it. |
| `src-tauri/src/profile_automatic.rs:861` | Source wake and retry | Reconcile after process, provider or profile changes. Outstanding vehicle and peripheral retries supply the next deadline for the current owner. |
| `src-tauri/src/profile_debug_controller/qualification.rs:158,308,317` | Fixture | An explicitly enabled desktop qualification run offers edges at Windows performance-counter deadlines. Two bounded lane workers submit available edges through the owner-fenced debug queue. The 30-minute mode writes bounded binary traces outside admission. |
| `src-tauri/src/profile_debug_controller/qualification_reducer.rs:51` | Fixture deadline | The qualification request waits for completion of the last expected reducer batch. An event signals completion, and a two-second deadline fails missing evidence. |
| `src-tauri/src/profile_game_launch.rs:339` | Owned startup deadline | Wait for a process observation, launch control notification or startup expiry. A spurious wake reuses the original deadline. The shared observer owns discovery. |
| `src-tauri/src/profile_game_orchestration_runtime.rs:380` | Operation deadline | Wait for the started process proof and ownership changes until the held launch expires. |
| `src-tauri/src/profile_input_learning.rs:503` | Source wake | Park when no active learning operation has capture work. |
| `src-tauri/src/profile_input_learning.rs:593` | Source wake and expiry | Wake for learning changes or the exact end of the 30-second operation. |
| `src-tauri/src/profile_publication_actor.rs:108` | Source wake | Wait for Start, Stop or retirement of temporary publication authority. |
| `src-tauri/src/profile_publication_actor.rs:441` | Lease and cleanup deadlines | Sequenced gateway ownership changes wake CAN publication. Renewal, original lease expiry and bounded cleanup can wake it at their deadlines. |
| `src-tauri/src/profile_publication.rs:708` | Operation deadline | Wait for a publication revision change or the caller's budget. |
| `src-tauri/src/profile_read_models/mod.rs:149` | Source wake | Wait for native read-model initialization to publish its readiness result. |
| `src-tauri/src/profile_read_models/mod.rs:195` | Source wake | Park until a committed inbox change or shutdown. |
| `src-tauri/src/profile_resources.rs:555` | Source wake and retry | Provider changes and projection revisions wake resource inventory. Retained evidence expiry and unresolved discovery or save retry may supply deadlines. |
| `src-tauri/src/profile_resources/host_observation.rs:53` | Source wake | Wait for host controller inventory or committed preference invalidation. |
| `src-tauri/src/profile_resources/host_observation.rs:115,116,119,120,122,143,145,149` | Fixture | Bound discovery and cached projection notifications and verify quiet periods. |
| `src-tauri/src/profile_resources/observation.rs:73` | Source wake | Park the resource publication worker until its committed source revision changes. |
| `src-tauri/src/profile_samples/registration.rs:59` | Operation deadline | Wait for the exact applied reader's initial sample baseline or its preparation deadline. |
| `src-tauri/src/profile_samples/worker.rs:79` | Source wake | Park the lazy applied-profile preview publisher until samples or owner scope change. |
| `src-tauri/src/profile_session_action_host.rs:88` | Operation deadline | Wait for exact physical action acquisition, binding or cleanup evidence until the operation deadline. |
| `src-tauri/src/profile_session_actions.rs:810` | Lease and binding deadlines | Physical action dispatch wakes for bind, work, Stop or the current lease renewal and bind deadlines. |
| `src-tauri/src/profile_session_companion_host.rs:417` | Operation deadline | Wait for producer changes or the panel preparation deadline while activation separately renews held outputs. |
| `src-tauri/src/profile_session_host.rs:304` | Operation deadline | Wait for exact native panel baseline publication until the activation deadline. |
| `src-tauri/src/profile_session_launch_host.rs:190` | Operation deadline | Wait for provider notification, validation or baseline deadline. Sources without a wake can retain a bounded fallback. |
| `src-tauri/src/profile_session_panels_inputs.rs:266` | Fixture | Bounded wait used by the synchronous panel test Start helper; live activation uses the independently woken baseline path. |
| `src-tauri/src/profile_session_panels_inputs/can_stream.rs:205` | Source wake and expiry | Wait for exact gateway CAN delivery or the original capture freshness deadline. |
| `src-tauri/src/profile_session_panels_inputs/can.rs:41,45` | Fixture | Legacy polled CAN adapter is compiled for test fixtures; retry and shared permit contention sleep 5 ms in that fixture. |
| `src-tauri/src/profile_session_panels_inputs/test_support.rs:61,68,160,355` | Fixture | Panel reader test barriers and authored delays. |
| `src-tauri/src/profile_session_panels_inputs/usb.rs:153` | Source wake | Wait for the subscribed USB input capture or scope retirement. |
| `src-tauri/src/profile_session_publication_host.rs:57` | Operation deadline | Wait for exact settled publication status or its activation deadline. |
| `src-tauri/src/profile_session_publication_host.rs:232` | Operation deadline | Wait for panel resource publication and exact route binding until its deadline. |
| `src-tauri/src/profile_session_visibility_host.rs:58` | Operation deadline | Wait for visibility activation or restoration progress while retaining the caller's deadline. |
| `src-tauri/src/profile_session/activation_heartbeat.rs:40` | Lease deadline | Keep the prepared virtual feeder neutral while activation work runs. Wait for the existing renewal deadline, feeder fault, admission cancellation, work completion or phase expiry. Prepared desktop scopes have no registration to renew. |
| `src-tauri/src/profile_session/activation_heartbeat.rs:236` | Operation deadline | Wait for acquired source and output readiness until the current activation deadline. |
| `src-tauri/src/profile_session/companion_feedback.rs:126` | Contention retry | The independent presentation worker gives one still-current panel update a single retry after a 2 ms Hub-lock backoff. Mapping and safety maintenance continue separately. |
| `src-tauri/src/profile_session/game_publication.rs:164` | Operation deadline | Wait for a returning reader's exact baseline or its deadline. The reader selects a bounded fallback if it cannot notify. |
| `src-tauri/src/profile_system_notification.rs:326` | Operation recovery | Recheck a missing Windows toast update for at most 50 ms on its worker so an in-flight dismissal can arrive. |
| `src-tauri/src/system_inventory.rs:66` | Source wake | Wait for committed topology, output or exact optional driver file invalidation. |
| `src-tauri/src/system_inventory.rs:323,329,354,362` | Fixture | Bound inventory commit and slow-provider isolation checks. |
| `src-tauri/src/system_inventory/driver_watch.rs:153` | Fixture | Bound the wait for real Windows notification on a private fixture file. |
| `src-tauri/src/system_inventory/vjoy_status.rs:141` | Approved acquisition | One Windows owner queries status and owner PID every 500 ms while visible status readers exist. Reader lifecycle and cache changes wake the owner. A helper failure uses a three-second retry. With zero readers the owner parks without a timer. |
| `src-tauri/src/window_persistence.rs:12` | User debounce | Coalesce window moves and resizes before saving the latest geometry. |
| `src-tauri/src/window_persistence.rs:50,54,57,65` | Fixture | Bound window geometry coalescing and shutdown assertions. |
