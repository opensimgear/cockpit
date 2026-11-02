# Controller preview capture isolation — 2026-11-02

R2 #191 / parent #188 remain in progress. The user reported latency in the
controller preview. This milestone removes preview JSON and broker work from
the authoritative controller registry mutex, which also protects routing reads.
It does not establish raw-input-to-preview or virtual destination latency.

## Behavior and bounds

Native reports commit to the existing typed journal and wake profile routing.
Selected preview sources enqueue typed immutable observations to one lazy,
event-driven publication worker. That worker builds JSON, registers topics and
publishes full snapshots/ordered changes outside the capture mutex. Descriptor
arrays are shared per native generation/fingerprint. Capture still decodes and
copies bounded sample maps; it does not wait for a UI subscriber or publisher.

The FIFO holds at most 128 entries / 1 MiB of accounted payload. Overflow removes
covered entries for that source, retains one bounded latest recovery snapshot
and causes explicit stream loss followed by resynchronization. Recovery and FIFO
service alternate fairly. Each typed source snapshot is limited to 256 KiB;
oversized state withdraws its exact preview registration. There are at most 64
published sources and 64 additional bounded pending preparations; recovery slots
are bounded by those sources. No ordered input is silently declared continuous
after queue loss. The authoritative routing journal is independent of this queue.

Subscription preparation captures a typed boundary under the registry mutex,
then waits and subscribes after releasing it. A pin prevents idle retirement or
idle encoding deferral between readiness and opening the reader. Broker snapshot
and listener registration remain atomic. A separate observation epoch orders
metadata, report and identity changes without renewing capture evidence.

Speculative registry clones retain no publication authority. Shutdown permanently
closes preparation, wakes waiting callers, withdraws exact topics and joins the
worker outside the capture mutex. A stopped service cannot recreate the worker.
`Broker::retire` closes existing readers and cannot remove another registration
that reused the topic name.

## Verification

- Isolated controller suite: **136 passed**, zero failed. Twenty-four observation
  checks include blocked publication with continuing capture/metadata, two-reader
  overflow recovery, metadata-only fences, registration pins, fair draining,
  shared descriptors, terminal withdrawal and shutdown. All controller module
  line gates pass 71%; queue **97.58%**, worker **92.19%**, observation facade
  **94.74%**, service **80.07%**. Coverage uses the shared production-source parser,
  including production methods after interleaved test helpers. Its 11 regression
  tests pass.
- Portable streams: **38 passed**, all 11 module gates pass; broker **98.45%**.
- Isolated native suite: **1,816 passed**, zero failed, 17 explicit helper/opt-in
  checks ignored; 97.91 seconds with four workers.
- Isolated `src` frontend suite: **1,539 passed**, zero failed. Focused controller
  observation suite: 14 passed. Svelte check: zero errors/warnings. Native and
  Vite production builds pass; existing build-size advisories remain.
- Actual Windows Cockpit, **22:33–22:35 UTC**: the shared physical-controller
  subscription/reopen fixture passes with no controller-inspect polling. The
  profile editor's physical VKBsim Gladiator EVO R inspector contains all 137
  controls; all six rendered axis values match the shared native observations and
  retain their declared native ranges. Source identity is checked explicitly.
  No profile owner or physical/virtual output was activated. The passive fixture
  was removed afterward.
- Native executable SHA-256:
  `d1b0f53910d261fad497ef6773fc5d16ca74c59e014ab715632c192bf38edd15`.

The dev server was restarted before final profile inspection because its ignored
research-directory watch had retained an earlier inspector module. The final
check uses the staged source identity marker and a freshly served module.

## Bounded admission diagnostic

An isolated debug build of `report_cost` admits 137-control synthetic reports.
Each open-preview report waits for its corresponding change outside the measured
capture interval and checks exact native sequence/value. After 100 warmups,
500 report calls are measured per scenario:

| Scenario | Before p50 / p95 / p99 (µs) | After p50 / p95 / p99 (µs) |
| --- | ---: | ---: |
| Preview never opened | 248 / 347 / 437 | 246 / 286 / 370 |
| Preview open | 2708 / 2991 / 3661 | 287 / 474 / 520 |
| Opened then closed | 248 / 293 / 394 | 333 / 472 / 499 |

These are report admission costs, not end-to-end latency. The worker's encoding,
IPC, OS/HID delivery, virtual dispatch and display are outside the measured
interval. The closed-preview actor still handles typed updates; further removal
of unused work remains open. There is no claim of sustained 1 kHz end-to-end
delivery from this diagnostic; overload semantics are tested separately.

## Timing and remaining qualification

`scripts/reactive/profile-preview-latency.mjs` measures the actual profile
inspector, requires its exact source identity and continuous native/wire sequence,
and brackets the native clock before/after measurement. It excludes ambiguous
repeated values and requires at least 20 distinct matched axis transitions in a
visible, focused window. Stationary-controller runs correctly fail qualification;
they cannot establish when an already-visible value arrived. The initial local
prototype's stationary-value timing result is discarded.

The connected controller remained stationary, so no new physical preview latency
percentiles are qualified. Deliberate controller sweeps, raw capture timestamps,
virtual destination receipt, simulator effects and hardware load remain in #199,
first in progress. Existing service/provider polling and complete-app cutover
remain open. Earlier Devices-inspector measurements do not substitute for these.

Local artifacts: `.research/reactive/r2-preview-*`,
`.research/reactive/controller-admission-{before,after}-actor.json`,
`.research/reactive/profile-inspector-after-actor.{json,png}` and
`.research/reactive/profile-preview-latency/`. Controller coverage is in
`.research/controller-coverage/`.
