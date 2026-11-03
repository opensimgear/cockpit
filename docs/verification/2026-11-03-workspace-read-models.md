# Native workspace ownership and passive subscriptions

This is a bounded R5/R7 delivery for #188, with MCP parity under R8. It does
not complete those milestones or qualify physical gateway timing.

## Behavior

- Native startup owns gateway discovery, automatic connection, guarded legacy
  startup and missing configuration/action metadata. Subscription lifetime does
  not own these operations. Jobs are bounded to four globally and one per row.
- Native Windows CM, Linux udev, macOS IOKit and Unix local advertisement
  notifications invalidate discovery. Registration precedes the first baseline.
  Failed notification ownership is explicit; no enumeration interval is added.
- `workspace.state` publishes a shared cached projection, with bounded immutable
  digest pages for large records. Public ownership hints hash private client IDs.
  Passive pages neither query gateways nor renew authority.
- App drops its three-second workspace refresh. Configuration editing renews
  only already existing, unexpired claims through a separate bounded command.
  Renewal survives display loading/loss while editing continues, never acquires
  claims, and cannot renew output authority or manufacture source evidence.
- Explicit Disconnect holds automatic work without withdrawing connection
  ownership before acceptance. A rejected command preserves the prior policy.
  Command epochs fence previously queued automation; accepted Disconnect pauses
  subsequent work. Removed sessions withdraw old caches and ownership.
- Retry fences survive expiry during publication. All existing adapters receive
  their native ownership flag before the bounded scheduling pass.

## Local verification

The isolated candidate excludes unrelated pre-existing worktree changes.

- 361 shell and 181 device frontend tests, configured coverage tasks and all
  12 Nx ownership checks pass. Svelte reports zero errors/warnings. Production
  frontend and native builds pass.
- Workspace client coverage: 99.22% lines / 89.51% branches / 89.19% functions
  for GatewayWorkspace; 91.89% / 93.27% / 90.91% for the shared read model.
- Native tests cover passive caches, lease expiry, private ownership, command
  ordering, rejected Disconnect, session recreation, pause-on-arrival, retry
  expiry, immutable pages, MCP authorization and profile-lock independence.
- The rebuilt actual Cockpit passes two shared consumers, cached identity after
  unsubscribing one consumer, and 6.5 seconds without frontend gateway polling.
  This machine has zero matching gateway ports, so this proves the real native
  subscription path and idle behavior, not hardware connection transitions.
- Actual authored pulse deadlines, script completion/freshness and shared
  virtual-preview/Stop regressions pass. No external outputs are activated.

The final complete native suite passes **2,085 tests**, zero failures, with 17
existing explicit ignores. All nine changed native module coverage gates pass:
workspace control/policy/publication 100%, worker 97.48%, STM32 cache/renewal
95.18%, Pico boundary 71.43%, notification wrapper 100%, directory owner 85.06%
and Windows notifications 95.56%. All six coverage-helper tests pass.

The first full-suite run exposed an old catalogue assertion allowing only the
debug-input command to bypass the operation ledger. The assertion now explicitly
checks both debug input and lease renewal, including their required fields; the
complete final suite passes. The final coverage source stamp includes renewal
tests and the gateway/MCP admission boundaries. Test-only Windows annotations
use the coverage parser's explicit `cfg(test)` form so test lines are excluded.

All 1,079 staged source/config records match the tested isolated candidate after
line-ending normalization; all 140 unrelated dirty files remain byte-identical.
Actual Cockpit executable SHA-256:
`F32E675890FC18D540903BF21F0E6F6AE2F35D96A114B1F2AE1876A6557B247C`.

## Corrections and limits

Initial frontend failures were a cross-realm Error message and a test attempting
to recover a changed producer identity without reopening its subscription. Error
text is preserved structurally; producer identity fencing remains strict.

The first notification test attempted to rename a watched parent on Windows.
That replacement scenario belongs to Unix advertisement discovery; it remains a
Unix check. Windows exercises native CM registration/callback/shutdown and shared
file notification/shutdown checks. Linux/macOS build/runtime qualification remains
open; those platform implementations are not claimed tested on Windows.

Firmware telemetry acquisition cadence is unchanged and remains R2 work.
Other device/visibility providers, companion/settings/activity streams, remaining
integration/context/routing work, full UI/MCP acceptance and integrated R9 remain
open. User-operated physical gateway/simulator/latency acceptance stays in #199,
verified first in the progress lane. CI is deferred until all features are
implemented, as requested. No replacement overall percentage is asserted.
