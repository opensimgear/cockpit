# Direct USB input streams

This delivers the direct USB input slice of #188/R2/R4/R7. It does not complete
those milestones or qualify physical input latency.

## Behavior

The standalone USB service owns a blocking duplex receiver. Configuration and
action replies share its framing actor with unsolicited firmware input frames;
waiting for a configuration reply does not pause input delivery. Subscription
installation follows an unsubscribe acknowledgement so bytes from an earlier
subscription cannot become the new connection's baseline. Unsupported firmware
reports unavailable input rather than falling back to acquisition queries.

One connection-scoped native capture supplies profile readers and passive
inspection. It preserves ordered edges, assembles complete event pages before
publication, retains the original device capture milliseconds and native RX
Instants, and publishes explicit loss on faults, gaps and expiry. Its bounded
journal admits eight subscribers/watchers combined, 32KiB snapshots/frames,
64 pending frames and 512KiB pending bytes per reader. A batch admits at most
four pages/64 edges. Overload never silently coalesces native occurrences.

Profile USB workers watch before subscribing and park on native notifications.
Their mailbox wakes the routing worker immediately. They do not call USB
EventsState, EventsRead or Refresh, acquire a configuration gate, or renew an
editing/output lease. Baselines retain held levels without event occurrences;
fresh available axes are usable immediately at their original witness.

Raw firmware time and native time remain separate domains. Native batches carry
the exact advancing-sample Instant also used by the current snapshot. They never
reconstruct it from the wire's floored microseconds. A deterministic fractional
clock regression covers the inactive-update/activation race that previously
could reject a valid baseline as 1us backwards. Complete profile batches use a
conservative native first-observation witness, preserving ordered occurrences
without assigning cached axes a newer timestamp. Source freshness is not renewed
by repeated firmware timestamps; capture expiry remains 400ms and profile
admission rejects evidence older than 250ms.

The shared `usb.inputs.<SHA-256 of exact endpoint key>` broker bridge serializes
off the receiver thread and coalesces only absolute UI metadata. It admits four
endpoint bridges, resolves already-known keys without discovery or opening a
device, preserves native capture clocks, and handles source replacement, rejected
subscriptions, slow readers, last-reader retirement and shutdown. Native UI and
browser-dev subscription dispatch share the bridge. MCP audience eligibility is
declared; full negotiated MCP client qualification remains R8 work.

USB details now use that subscription instead of a 750ms EventsState interval.
Input updates cannot replace a local configuration draft or private claim
receipt. Only input from the exact acknowledged route is presented.

## Local verification

- Final instrumented native library suite: **2,159 pass, zero failures, 17 existing
  explicit ignores**. Final executable is the one pinned in the coverage evidence.
- All **24 changed native production-module coverage gates pass**. The decoder
  reaches 100%; Windows serial I/O reaches 81.58%. Separate Rust `mod` files are
  explicitly gated rather than hidden behind facade-only coverage.
- Six native coverage-helper tests pass. Source/test stamps remain pinned through
  measurement. Native regressions include actual C-generated packets over native
  Windows socket I/O, input during pending configuration, independent capture
  readers, reconnect isolation, exact fractional clocks, blocked-I/O cancellation,
  and real Windows overlapped pipes. Pipes do not qualify serial DCB/control lines.
- USB frontend tasks: **13 command-client tests and 19 observation/UI tests pass**.
  Command-client coverage is 100% lines/97.65% branches/96.88% functions;
  observation coverage is 100%/99.32%/91.67%. Svelte reports zero errors/warnings;
  production frontend and native builds pass.
- Actual rebuilt Cockpit: USB details reject an unobserved source, retain no live
  values and issue no input queries/authority commands across the removed interval.
  Shared preference delivery/reload/disposal passes. All four existing native
  pulse, script-completion, physical-reader replacement/Stop and profile-preview
  fixtures pass. These fixtures do not author external physical outputs.

The final tested dev executable SHA-256 is
`43c174ed55c0661bf76a5cba11f08385105ddb7ed50fd30ad2c1b63873d32ee0`.
Local evidence is under `.research/reactive/usb-input-coverage/`,
`usb-final-native-tests.log`, `usb-final-routing-tests.log`,
`usb-final-observation-tests.log` and `usb-native-preview.json`.

Earlier qualification exposed and corrected missing worker wake binding, an
unrelated-reader retirement bug, replacement-service watcher registration and
the exact-clock race. An initial fixture expected an edge-style change for a
coalesced UI snapshot; its assertion now follows the absolute-state contract.
Initial runtime runs also hit launch readiness and a bare browser module import;
the final runs use the loaded runtime and a fully started Cockpit. Successful
reruns do not replace the recorded causes or physical qualification.

The isolated candidate excludes unrelated work. All 138 unrelated files outside
shared paths remain byte-identical. Pre-change snapshots of the two shared paths
(`package.json` and `profile_session_host.rs`) match their original unrelated
hashes; staging uses the isolated merged blobs.

## Remaining qualification and original scope

The producer is delivered in firmware-common `a46a814`, device-template `146b3a9`
and protocol `e36771b`. No board was flashed as part of this host verification.
Actual USB serial configuration, board capture-to-preview/virtual timing,
suspend/resume, input during storage and other physical effects remain in #199,
verified first in the project's progress lane. No physical latency distribution
or uninterrupted collection during firmware flash writes is claimed.

CAN input push, two-second USB status/config metadata queries, broader device
lifecycle/read-model migration, full MCP negotiation and integrated R0–R9
qualification remain open. The input stream cannot substitute for owner-expiry
or external configuration notifications. Linux/macOS build/runtime qualification
is not established by this Windows run. CI remains deferred until the requested
feature implementation is complete.
