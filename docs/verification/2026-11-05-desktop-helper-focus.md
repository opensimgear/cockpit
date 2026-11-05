# Native desktop helper scope notifications

## Delivered boundary

The private native worker no longer samples focus every 5 ms. Foreground,
desktop and session notifications wake its bounded scope registrations directly.
Native queue arrivals and the nearest live lease or release guard are the other
wake sources. With none outstanding, it waits indefinitely for an event.
Recording adapters retain explicit fake-state sampling for portable tests.

Every Activate establishes a channel-registration identity and initial epoch.
Arm, delivery and pointer jobs carry the original immutable admission proof.
Retained loss evidence advances the helper epoch even across an away/return
transition. Stale jobs are skipped without disturbing a newer valid hold.
Immediate native eligibility checks remain before acquiring submissions.
Returning to eligibility requires a fresh neutral baseline and a fresh Arm.

Acknowledgements carry a complete bounded scope-receipt vector. The parent
validates registration membership, uniqueness, monotonic epochs, expiry and
eligibility before publishing any receipt. It reconciles cross-run losses and
release receipts before accepting held projections. Validated loss revokes
queued authority before final journal persistence. Malformed acknowledgements
or observer failure retain the prepared request and conservative possible holds
for explicit recovery, including when a forged reply claims no native holds.

Expiry releases contributions before tearing down native subscriptions. Expired
registrations survive through one acknowledgement as bounded tombstones.
Concurrent final-subscription retirement and replacement rendezvous with actual
native observer exit outside the registry lock, within the existing deadline.

The read-only desktop runtime status exposes the live helper scope receipt;
the same gateway state is available to portal and MCP consumers. Stop clears it.

## Local verification

An isolated candidate starts from main 5ea28077 and overlays only this milestone.
All 108 pre-existing unrelated file hashes are preserved.

- 182 desktop-output tests pass, including forged/missing proofs, stale queued
  jobs, cross-run projection rebasing, recovery, blocked persistence, expiry
  teardown ordering and native observer retirement races.
- 160 mapping-host tests pass, including helper-only loss while parent scope
  evidence remains unchanged and immediate queued-authority revocation.
- The full native suite passes: 2,289 tests, zero failures, 17 existing ignores.
- All 49 affected production coverage gates pass (28 desktop, 21 host).
  New helper scope modules have 100% line coverage; coordinator scope validation
  has 95.8%, transaction handling 90.37%, worker scopes 95.92%, and observer
  lifetime/registry modules 100%/94.74%.
- Coverage parser regressions and the original 247-entry timing inventory remain
  checked separately. Production Cockpit and the native receiver build.

## Running Cockpit evidence and limits

The audit-disabled actual runtime is PID 47972, executable SHA-256
`0632cbc50d89b7c83dc61b9a5f987fcbf9978a56bad33ae3f109fdc8b27e5eaf`.
The receiver SHA-256 is
`b43f4ca2a98eaa2a21f880399164b8ea634dafe377fececcd4d729516aedfc1e`.

Three actual-Cockpit checks pass: native scope registration/replacement/Stop,
physical source reader activation/replacement/Stop without previews, and shared
controller capture observer close/reopen. Scope replacement produces distinct
registrations and Stop removes the helper receipt. Final cleanup has no active
profile owner, no held or possible-held inputs, uncertainty false and all three
virtual slots free.

The receiver now has a keyboard-only mode that avoids cursor positioning and
requests ordinary foreground activation. The controlled two-window focus run
still cannot establish its required foreground window in this session, before
authored input begins. A computer-use attempt also fails during backend
initialization with a missing kernel-assets path, before any desktop action.
Neither result proves a locked desktop or qualifies native focus transitions.
That software qualification remains open under #193/#198.

## Remaining original work

The host still services desktop routing on its 5 ms cadence. In addition,
`src-tauri/src/gateways.rs` still observes foreground identity on profile-worker
iterations for the global recent-application chooser. Both require the remaining
host acquisition cutover; removal of helper polling does not remove them.
First-dispatch and burst timing, controlled focus and mixed fault/load matrices
remain open. Physical preview, raw-input-to-virtual and simulator effect timing
are separate user-operated scenarios in #199. This milestone establishes no
physical latency percentile and closes no original acceptance criterion alone.

CI is deferred until feature implementation is complete, as requested.
