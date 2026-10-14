# Controller mapping contract candidate (M0)

This is the version-1 contract candidate for [Cockpit #21](https://github.com/opensimgear/cockpit/issues/21).
It is exercised by the bounded experiments in [.research/controller-mapping-m0](../.research/controller-mapping-m0/README.md).
It does not expose controller mapping in the application or qualify a platform backend.
M0 remains open until the required physical/provisioning/visibility gates in the
[qualification record](verification/2026-10-14-controller-mapping-m0.md) pass.
Do not claim that the release contract or those backends are frozen/delivered yet.

## Independent features and readiness

Persist separate user enablement flags for virtual devices and device hiding, both
false by default. For each backend, expose one of: missing, unsupported,
setup-required, reboot-required, ready, or lost. Discovery only observes readiness;
installation, discovery, import and ordinary startup never enable a feature.

Activation requires explicit enablement, ready state and the exact requested
capability. Preview/recording requires neither optional integration. One ready
integration does not make the other ready. A backend that supports input suppression
must reject a request for enumeration hiding or process-specific filtering.
Dependency loss stops only dependent sessions and invalidates their generations.

## Bounded local messages

All lengths below are UTF-8 byte lengths. Schema numbers are integers. Reject
unknown versions, duplicate JSON keys, non-finite numbers, invalid UTF-8, invalid
field types and excessive nesting before activation. Parse only after enforcing
the frame limit; do not read an unlimited line and check it afterward.

| Item | Bound |
| --- | --- |
| Portable mapping document, encoded as a JSON object | 65,536 bytes |
| Source roles / logical virtual targets / mapping nodes | 16 / 8 / 256 |
| Script source inside the document | 16,384 bytes |
| Request correlation ID | 1–80 UTF-8 bytes |
| Document request frame, including metadata | 66,560 bytes |
| Status page | At most 32 records and 65,536 bytes; explicit cursor |
| Normalized controls per source / updates per atomic report | At most 256 / 256 |
| Script state / output intents / timers per script | 16,384 bytes / 128 / 64 |
| Timers per run | 256 |
| Script request/reply frame | 32,768 bytes, including state and output |
| Worker in-flight callbacks per script | 1; at most 32 queued callbacks per run |
| Source transition queue | 1,024 reports; overflow faults and rebaselines that source |

A document transfer has this shape:

~~~json
{"schema":1,"requestId":"example","document":{}}
~~~

The document remains an object, not a JSON string that doubles escaping overhead.
Envelope and document bounds both apply; using all of one limit never grants an
exception to the other.

The current gateway HTTP body limit is 65,536 bytes. A maximum-size mapping
document plus an envelope **cannot** be sent as a gateway request unchanged.
M1/M2 must add a separately bounded controller endpoint or bounded upload operation.
Keep current gateway limits and correlation behavior unchanged.

Use canonical decimal strings for nonzero u64 generations, sequences and monotonic
microsecond counters at JavaScript boundaries. Reject signs, leading zeroes,
non-ASCII digits and overflow. Do not round these values through JavaScript Number.
A worker/session UUID plus a generation prevents a process restart from reusing
an old incarnation. Clock values are relative to a session and never wall-clock dates.

## Sources and atomic reports

A descriptor carries a tagged source (host-controller, osg-panel-events, or
synthetic), a persisted Cockpit source ID, backend, local match evidence,
capability fingerprint and bounded control descriptors. A control descriptor retains
its backend control ID, native range, signedness, neutral/null behavior and normalized
kind. Preserve axis native values for diagnostics; use documented bipolar or unipolar
normalization for the engine.

OS device paths, handles and enumeration indices are observations, not identity.
Serial/interface/collection evidence takes precedence. Ambiguous devices require
explicit assignment. Exclude Cockpit-owned virtual outputs and reject provably
duplicate panel/host routes.

A report carries source ID, generation, sequence, capture time, reason and a complete
atomic update set. The initial baseline contains every advertised control.
A changed capability set requires a new generation. An event gap, disconnect or
backend overflow releases that source's contributions/timers and requires a fresh
baseline. Old-generation or duplicate reports cannot disarm or rewrite a newer run.
No incoming reports is not itself a disconnect: stationary devices retain state.

The prototype uses integer sample values solely to test framing/atomicity; it is
not the complete typed button/axis/hat parser. Linux SYN_DROPPED invalidates pending
changes and committed state. Ignore queued deltas until native state is reacquired;
SYN_REPORT is the atomic commit boundary. M1P must qualify native evdev behavior.

## Mapping documents and local assignments

The portable document contains schema, logical roles, logical targets, layouts,
nodes, modes, scripts/parameters, axis fallbacks, arbitration and optional-feature
intent. Semantic validation belongs to M2; the M0 envelope test alone is not a
mapping-schema validator. Imported documents never execute/provision/hide/start.

Keep local identities, source assignments, backend slots, final executable paths,
installed dependencies and enablement outside the portable document. One active
profile may own several targets. Prepare all requested sources, scripts and outputs
before replacing an existing run. Failure releases partial acquisitions; restoring
an old run requires fresh baselines and never replays gestures.

## Output and worker ownership

Every assertion, report, timer and callback reply carries session, run generation,
node/producer ID and target ID. Discard replies after Stop, replacement, source loss,
worker restart or an expired output lease. A callback commits replacement state,
intents and timers as one validated transaction; failure commits none.

Buttons combine per-producer holds using OR. Axes/hats have one writer unless the
profile declares an explicit merge or priority policy. Emit complete target reports.
Keep generated transitions separate, with initial minimum press/release durations
of 30 ms; game qualification must validate that default.

Initially held momentary controls require release before arming; explicit level
mappings may synchronize a baseline. Stop cancels pending actions, publishes
per-control fallback output, releases visibility, then relinquishes outputs.
Do not use a driver's configurable Reset operation as the definition of neutral.

The proposed feeder lease is renewed every 50 ms and expires after 250 ms.
The feeder rejects obsolete generations even after expiry. These are prototype
budgets to qualify under load, not measured guarantees. A supervisor may reacquire
a recorded vJoy slot only when FREE. Total process termination does not guarantee
vJoy neutralization; recovery needs an explicit documented procedure.

Rhai runs in a supervised process, with a raw engine and no file-module resolver,
device APIs or ambient OS/network/shell functions. Initial operation/call-depth
bounds are 100,000/16 and the evaluation deadline target is 5 ms. Bound compile
input, strings, arrays, maps, serialized state and output separately. Operation
counts are not milliseconds or a total memory budget. The M0 tests prove operation
termination and progress cancellation; they do not establish OS sandboxing or a
production worker supervisor.

## Exact visibility policy

HidHide's global application set and inverse flag apply to the whole hidden-device
set. Normal mode can represent a common hide-all policy with explicit capture and
existing application exceptions. Inverse mode can represent a common selected
executable policy. It cannot represent arbitrary different per-device policies.

Canonicalize existing executable paths through the OS before policy comparison;
persist paths, not PIDs. Case/slash normalization in the experiment does not resolve
links, short names or driver-specific path forms.

Reject incompatible device/process selections and any change that would alter an
externally hidden device's policy. Never select a Cockpit virtual target for hiding.
Show existing exceptions and externally managed hiding. The conservative prototype
refuses global changes when external hidden devices would be affected.

The selected signed HidHide release has persistent list/mode IOCTLs. Its Windows
logon-session filtering is not proof of process-owned automatic-unhide APIs.
Do not use newer master-branch session APIs with this binary.

Before persistent mutations, journal original state, intended exact post-state,
owned additions and mutation progress. Restore only owned deltas when current state
still matches expected state; external changes are a conflict, not permission to
replace the user's configuration. The pure prototype exercises conflict refusal;
the live Windows prototype additionally qualifies normal-mode DirectInput/HID
access and process-exit restoration for the selected specimen. Raw Input reports
remain visible to the non-allowlisted observer. Never advertise universal process
isolation from that result; qualify each supported input API. Inverse mode, reboot
recovery and full supervisor termination remain unqualified.

Linux EVIOCGRAB and macOS seizure are input-suppression capabilities, not guaranteed
enumeration hiding. Linux process isolation applies only to a qualified controlled
launch boundary; arbitrary existing PIDs are unsupported. macOS per-process
filtering is unavailable. Neither capability substitutes silently for another.
