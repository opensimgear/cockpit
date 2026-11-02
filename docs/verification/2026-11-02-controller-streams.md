# Controller stream verification — 2026-11-02

Scope: initial R2/#191 controller owner and inspector migration under #188. The
prior R1 commit `582a527` passed GitHub CI. R2 remains open. User-operated sweeps,
unplug/replug, simulator response and combined routing acceptance remain in #199,
first in the progress lane and awaiting the rest of the software prerequisites.

## Running native cockpit

The executable was built from an isolated staged tree, excluding unrelated editor
work: `655e0178d7e3b88662791725422588ee16662718`. Executable SHA-256:
`052c6af72c35ea35bb12990266276c5dc09f0b89b028599973dc6b4f8a2ed142`.
Subsequent changes refine the frontend distinction between subscriber loss and
device presence and add tests, measurement tooling and documentation. Native
production code is unchanged from that build.

The real Windows Raw Input owner observed **VKBsim Gladiator EVO R**, connected
with **137 controls** and a complete native baseline. The opt-in test
`node --test tests/ui/reactive-controller-native.mjs` passed. Two consumers shared
one native subscription, retained actual descriptor ranges and current values,
continued after the first disposed, and reopened after the last disposed. No
`controller_inspect` request occurred in the measured subscription interval.
The existing native preference observation/reload regression passed too.

The native app was then operated through Devices → VKBsim Gladiator EVO R. Its
inspector showed the assigned physical identity, current native axis values,
declared native ranges, hat state and buttons. No profile was active and no output
was enabled. With that view open, `node scripts/reactive/controller-ui-trace.mjs`
recorded five seconds: report **4119 → 4182**, **zero controller_inspect requests**.
There were 97 other application requests; this is explicitly not a whole-app idle
or zero-polling result. No deliberate physical sweep or input latency was measured.
Local JSON traces and the inspector PNG are in ignored `.research/reactive/`.

The first visible-view trace identified stale compiled inspector code in the
isolated Vite server: its research folder is excluded from file watching. That
server was restarted, its served module was checked for the new observation
consumer, and both actual native regression tests and the visible-view trace were
repeated successfully. Source checks alone were not counted as UI acceptance.

## Automated checks

- Controller library: **122 tests pass**. These cover atomic snapshots/reports,
  partial/full baselines, clone isolation, committed metadata, idle binding
  recycling, slow-reader overflow, reconnect/generation fencing, reassignment
  during persistence I/O, numeric exhaustion, real Windows idle wake/shutdown and
  cleanup errors. All production modules pass the 71% line floor. New registry
  input is 100%, observations 91.27%, metadata 87.01%; Windows worker is 72.89%.
- Streams library: **23 tests pass**, including pointer-safe idle retirement.
- Native observation tests: **13 pass**. The browser control boundary validates
  the principal and fields before selecting a controller topic. Native observation
  module coverage remains above the floor; service binding coverage is 96.67%.
- Full isolated native library suite: **1,658 pass, zero fail, 12 ignored**.
  Ignored checks require opt-in network, pinned archive, installed simulator/runtime
  or real-browser prerequisites; they remain unqualified rather than counted as
  passes.
- Full isolated frontend suite: **1,663 tests pass** with concurrency two and
  required local resource-server access. New reducer coverage is **99.50% lines,
  95.68% branches, 100% functions**. Host view tests exercise stream selection,
  continued delivery, mutation races and disposal; profile preview uses the same
  observation API while preserving separate editor work in the working tree.
  All 22 focused view/reducer tests pass after the final loss/presence distinction;
  transport loss withdraws usable samples without inventing a native disconnect.
- Isolated candidate: Svelte check has zero errors/warnings; 12 Nx plugin tests
  pass; production Vite build and native library/binary compilation pass. The
  production build retains the existing chunk-size advisory. A dirty-root Nx
  source-closure check fails on unrelated editor changes; the isolated candidate
  passes that same check.

Coverage source discovery now includes every extracted registry and Windows worker
module, each with its own floor. Workspace tasks include the new frontend reducer
coverage and the native streams dependency.

## Unqualified paths

The Windows notification acquisition change and passive native inspector are
verified here. Browser controller transport execution, profile reducer wakeups,
inventory/visibility owner notifications, Linux/macOS capture, device-gateway and
firmware USB/CAN streams, full MCP observation, real edge/encoder stress, hardware
disconnect/reconnect and R0 latency/idle/soak budgets remain open. Existing routing
still reads the shared journal; this change does not claim it is event-driven.
