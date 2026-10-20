# Profile list and native activation progress

The profile list refreshes every three seconds while visible and immediately on window focus or visibility changes. Passive refresh preserves dirty editor drafts and ignores overlapping reads. The redundant Refresh button and global discovered-component count are removed. Import profile appears immediately before Add profile.

Manual activation receives a fresh operation UUID. Independent native progress and cancellation commands bypass the profile admission/coordinator locks. Cancellation revokes only that operation's captured admission epoch; a stale cancellation cannot deactivate a later owner. Read-only preparation cancellation preserves the previous owner. Cancellation after takeover waits for the existing compensation path and reports whether recovery is required. If activation commits before cancellation takes effect, the UI reports that the profile became active.

The native Cockpit opens an owned, nonmodal Tauri window. It displays target-specific companion preparation, cache/download byte progress and neutral-input readiness. Its Cancel button, Escape and window-close action submit the displayed operation's identity. The main controller remains the sole activation owner; the child window has no gateway permission. The window stays visible until activation or cancellation cleanup settles. Browser use and native-window failure fall back to the in-page dialog.

Native window commands fence stale updates, cancellation and close requests by operation identity. Adapter tests cover browser timer receivers, failed creation/update cleanup, cancellation retry and stale callbacks. The Debug view shares the runtime controller without creating a second progress-window owner.

## Automated checks

- The targeted profile runtime, controller, window-adapter, workspace and editor suite passes all 102 tests, including 11 window-adapter tests.
- Runtime controller coverage is 97.14% lines, 91.23% branches and 100% functions, exceeding the unchanged 71% gates.
- `cargo test --manifest-path src-tauri/Cargo.toml profile_activation --lib` passes 10 tests. `cargo test --manifest-path src-tauri/Cargo.toml tracked_ --lib` passes 4 tests. These cover cancellation during blocked preparation, responsive progress/cancellation while takeover locks are held, retained previous ownership, stale/malformed identities and cancellation at the successful commit boundary.
- The native checks include window cancellation intent, failed-cancel retry and stale-operation fencing.
- `pnpm check` passes for Cockpit and browser companion with zero errors and warnings. `pnpm ui:build` passes.

## Physical acceptance

`node --test tests/ui/profile-activation-progress-native.mjs` passed in 38.8 seconds against the actual dev Cockpit, its native child window and the paired Sunton USB screen on firmware `2026.10.20.1`. No IPC or companion was mocked. Evidence and screenshots are in `.research/profile-activation-progress-native/`.

The list observed externally created/deleted profiles automatically, and the actual toolbar placed the icon-bearing Import action immediately left of Add. The native window displayed the exact companion's cold image download (`0 / 78 KiB` at the captured frame). Clicking its actual Cancel button reached the exact backend operation; cancellation completed in 226 ms. This cancellation occurred after takeover began, so the native coordinator correctly required recovery. The main page displayed a single friendly cancellation message with Recover at the right.

After explicit recovery, a new activation UUID retried successfully. The physical screen downloaded all 79,200 raster bytes with one cache miss, reached ready with a fresh session and neutral baseline, and became owned by the profile. The progress window closed. Deactivation restored the original panel and the final native status had no owner and no fault. Temporary profiles and panel publications were removed.

During qualification, a prior failed input acknowledgement left the physical companion unarmed. A subsequent activation correctly rejected that missing verified route during preflight. Native stack sampling and independent status requests found no new tracker deadlock; an intermittently unresponsive CDP HTTP endpoint affected the diagnostic client separately. Restarting the dev Cockpit restored a fresh ready companion session without an explicit reconnect. The final run started from that verified baseline.

The hardware run used the actual profile/companion stores with only game setup temporarily isolated through the existing `OSG_GAMES_CONFIG` override, avoiding the previously documented ACR executable hashing overhead. Persistent game setup was not changed. The ordinary dev launch was restored afterward; its actual native status had no owner or fault, and the Sunton had its original unowned panel ready and armed with a fresh matching session.
