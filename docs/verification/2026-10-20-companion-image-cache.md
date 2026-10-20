# Shared ESP32 image cache and operation feedback

The existing `images` partition remains `0x260000` bytes (2.375 MiB). No partition, application or NVS allocation changes are required. The previous two publication-sized slots are replaced by a shared cache keyed by validated source asset SHA-256, exact rendered dimensions and pixel format version. Panels and repeated image widgets can reuse the same raster; a title or control-layout edit no longer invalidates unchanged images.

Entries have sector-rounded data, validated metadata and pixel CRCs. A new entry becomes reusable only after its full download passes flash readback. Incomplete or corrupted entries become misses. Persistent recency records support least recently used eviction without rewriting pixel data on cache hits. The index is bounded to 128 entries and each panel retains its 1200 KiB raster budget.

The displayed panel and incoming panel pin their assets. Incoming hits are pinned before allocating misses, so eviction cannot discard an image needed later in the same switch. Failed or cancelled candidates release their reservations while the displayed panel stays protected. Protected assets and fragmentation can leave insufficient contiguous space; that produces an explicit cache failure instead of erasing active pixels. Images remain flash mapped, including on the Sunton without PSRAM. Sessions, panel documents and input authority are acquired afresh from Cockpit.

Companion operation reports identify the target instance/publication, attempt and session, phase, completed/total raster bytes, cache hits/misses and failure code/retryability. Full nonce-bound USB status and authenticated image/heartbeat/baseline frames carry this metadata. Cockpit rejects mismatched or obsolete reports, and reported readiness never substitutes for the real publication, capabilities and neutral baseline. A recent USB report can explain a failure before session-open: it must match the current assignment and exact companion, come from one connected ready link, and have a locally measured full-status age under 1500 ms. Operation history is captured before assignment acquisition so a fast new failure is retained while an unchanged old failure cannot reject an explicit retry. Without any prior verified history, the first observed failure remains context until a new attempt or transition is observed.

Profile activation fails promptly on a reported terminal failure or loss of a previously observed target session. Validated progress has a 30-second idle limit and a five-minute overall bound; legacy firmware retains its 60-second bound. Output renewal and cancellation continue during the wait. Disarmed cache lookup/download/rendering has a bounded 30-second host preparation lease to accommodate synchronous flash work; armed inputs retain their existing 1500 ms lease. The USB response watchdog likewise permits 30 seconds only when the exact current USB connection has a validated disarmed preparation session; its normal five-second bound remains unchanged, and another Wi-Fi session cannot extend it. USB reconnect/disarm/forget can target an exact companion independently of the currently selected USB port.

Companion tiles present progress and actionable failures through their existing message hover icons. Profile Debug includes live companion operation/cache details, polling only while those details are open.

## Saved panel behavior

Panel selectors show a single latest saved entry per stable panel identity, without revision or publication-hash labels. Immutable content hashes remain internal transport identifiers. Existing profile selections migrate to that stable identity, and saving a panel advances references in saved profiles. Active profiles use the normal journaled apply path to acquire the new view and a fresh neutral baseline; incompatible control changes produce an actionable error while preserving the previous active execution.

Transport archive restoration cannot roll back the saved state. A clean open profile editor refreshes when the saved panel changes, while unsaved edits remain intact. Deactivating a profile restores the latest explicitly saved view of its original panel only after verifying the exact protected ownership receipt. Independent assignment updates also compare the original unowned assignment atomically.

Background library uploads and identical saved-head restoration do not interrupt profile activation or input learning. Upload staging does not hold profile admission; only a changed view used by an active profile allocates a new application epoch, after checking that deactivation or a newer control has not superseded the Save.

## Automated verification

Commands run from the repository root on Windows:

| Command | Result |
| --- | --- |
| `cargo test --manifest-path crates/companion-service/Cargo.toml` | 111 unit and 2 integration tests passed, including actual local TLS/persistence, operation identity, neutral-baseline admission, preparation expiry, unchanged armed leases, latest saved restoration and atomic unowned assignment comparisons. Required normal-user temporary-file and loopback access. |
| `cargo test --manifest-path crates/companion-serial/Cargo.toml --lib` | 19 passed, including independent USB workers, exact companion command routing, bounded diagnostics, stale-session rejection and suppression of repeated terminal-failure retries. |
| `cargo test --manifest-path src-tauri/Cargo.toml profile_session_host::companion::tests --lib` | 14 passed, 2 ignored. Includes native activation failure propagation, pre-open retry fencing, cancellation, actual generation/baseline admission and restoration. |
| `cargo test --manifest-path src-tauri/Cargo.toml pre_open_ --lib` | 2 passed. Covers exact USB assignment/companion/freshness checks and pre-open failure/retry handling. |
| `cargo test --manifest-path src-tauri/Cargo.toml saved_panel --lib` | 4 passed. Covers bounded head storage, stable saved heads, inactive-profile propagation, archive restoration, actionable incompatible rebinds and identical-Save retry against an older active owner. |
| `cargo test --manifest-path src-tauri/Cargo.toml archived_snapshots_do_not_establish_saved_restoration_heads --lib` | 1 passed. Retained archive-only panels cannot become latest saved restoration targets. |
| `cargo test --manifest-path src-tauri/Cargo.toml editor_save --lib` | 9 passed. Includes cancellation, responsive deactivation/recovery and newer-Save fencing. |
| `cargo test --manifest-path src-tauri/Cargo.toml panel_ --lib` | 24 passed. Includes background library restoration during blocked activation, responsive status and rejection of delayed panel application after deactivation. The background-restoration regression passed again with the final transport admission-lock exemption. |
| `cargo test --manifest-path src-tauri/Cargo.toml input_learning_ --lib` | 4 passed. Existing output, pending-activation and cancellation boundaries remain enforced. |
| `cargo test --manifest-path src-tauri/Cargo.toml companion_host::authoring --lib` | 6 passed. The catalogue exposes one latest saved panel per identity, retaining internal old-hash aliases for migration. |
| `node --test src/lib/companion-messages.test.mjs src/lib/profile-workspace-ui.test.mjs` | 36 passed, including USB/network progress, nonduplicated actionable failures and profile editor/workspace behavior. |
| `node --test src/lib/profile-editor.test.mjs src/lib/profile-workspace-ui.test.mjs` | 53 passed. Passive saved-panel refresh updates clean editors and preserves dirty drafts and edits made while a refresh is pending. |
| `pnpm check` | Cockpit and browser companion checks: zero errors and warnings. |

The sanitizer-backed pixel-cache harness covers shared assets, dimensions, LRU eviction, active/incoming pins, interrupted/corrupt writes, restart recovery and storage failures. The consolidated runner `python3 apps/companion-esp32/test/network.py` passed under WSL. Final cache coverage is 99.03% lines, 74.58% branches and 100% functions; client coverage is 96.46% lines, 72.19% branches and 97.67% functions. Every module exceeds the unchanged 71% per-metric gate. Evidence is recorded in `.research/esp32-network/coverage.json`.

The profiles crate's 121 unit tests and 5 editor-metadata integration tests passed. The broader `complete_rig` integration suite had 3 passes and one failure at its offline CAN-resource expectation (`tests/complete_rig.rs:298`); that fixture was left outside this change.

## Physical qualification

Installed firmware `2026.10.20.1` on the paired Sunton on COM8 through the running native Cockpit's update flow. Setup was preserved and the partition-table SHA-256 remained `4c371bf1aef93f2aed6f99bec8b6f8e16c6b71f47e7fef6d6480c6b76591f369`. Both Sunton and Waveshare cross-builds fit their original 1.5 MiB application allocation (892,480 and 908,944 bytes respectively).

`node --test tests/ui/companion-image-cache-native.mjs` passed against the actual dev Cockpit and physical Sunton in 49.2 seconds. It published temporary panels, switched their real assignment, observed progress, established a fresh neutral baseline after each switch and restored the exact original panel. No profile remained active.

| Switch | Raster | Cache hits / misses | Time |
| --- | ---: | ---: | ---: |
| First title variant of the original image | 79,200 B | 1 / 0 | 2.98 s |
| Different panel with the same image and size | 79,200 B | 1 / 0 | 2.55 s |
| New rendered size C | 66,960 B | 0 / 1 | 19.56 s |
| New rendered size D | 55,440 B | 0 / 1 | 15.85 s |
| Return to A after C and D | 79,200 B | 1 / 0 | 2.67 s |
| Explicit reconnect with a new actual session | 79,200 B | 1 / 0 | 1.96 s |

The cockpit's existing companion message icon exposed real percentage/byte/cache progress; UI evidence and operation history are in `.research/companion-image-cache-native/`. Physical testing found that preparation ACKs initially consumed the feedback cursor before the new runtime could read it. The host now preserves that cursor during cache/download heartbeats, so the first rendered-panel ACK supplies a snapshot. Regression `preparation_heartbeats_preserve_the_initial_rendered_panel_feedback_snapshot` passes.

A normal hardware reset of the Sunton also preserved pairing and reused the cached image with a fresh session and neutral baseline (`reset-evidence.json`).

`node --test tests/ui/panel-save-active-profile-native.mjs` passed on the final native build in 11.3 seconds. Through the actual UI it verified one current panel option, migrated an older profile selection to the stable saved panel identity, activated the profile, changed a button label and title in the panel editor, and clicked Save. The physical screen acquired the changed publication in 3.94 seconds with a new session, a fresh neutral baseline and one cache hit with zero misses. Deactivation restored the original panel; the temporary profile and panel were removed. Evidence is in `.research/panel-save-active-profile-native/`.

The final warm-cache switching test passed in 24.5 seconds, including an explicit stop that remained stopped for over 4.2 seconds and an explicit reconnect with a new session. Restarting Cockpit also automatically reconnected once despite the device retaining the old host's shutdown error. New failures on that ready link still suppress repeated retries.

Repeated UI acceptance exposed and fixed a race where background panel-library uploads invalidated pending profile activation. It also exposed separate existing ACR executable hashing during game setup refresh (`acr_setup::fingerprint`, confirmed with native call-stack symbols). The final companion runs used the real dev Cockpit, its actual profile/companion storage and physical screen, with only game setup temporarily isolated through the existing `OSG_GAMES_CONFIG` override. No game integration source or persistent setup was changed.

Automated flash fault injection does not qualify arbitrary physical power loss, flash endurance or all board timing conditions. Physical Wi-Fi delivery, held-touch interaction and maximum-size real-panel transfer performance remain unmeasured. The cache remains bounded: protected assets are never evicted merely to make a switch succeed.
