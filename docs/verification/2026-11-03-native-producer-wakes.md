# Native producer routing wakes

Incremental delivery for #188/#190/#192/#193. Canonical DCS BIOS, shared DCS
ownship and X-Plane native commits now notify the profile worker directly, removing
the need to wait for its next 5 ms service opportunity when data arrives.
Remaining adapter/global-context service work is still explicit; this does not
claim fully tickless profiles or a measured physical latency improvement.

## Delivery and lifecycle

The shared NativeWake primitive contains an atomic notification epoch and one
bound thread handle. notify() only advances the epoch and unparks that thread:
no consumer callback, graph reduction, JSON/IPC work, payload queue or additional
polling worker runs on the producer. Observing before drain and checking before
park preserves notifications when another wait consumes a token. Binding preserves
pre-bind notifications without inventing an initial wake when no event occurred.
Counter exhaustion forces draining rather than wrapping into a lost notification.

NativeStream::watch registers a bounded passive RAII lease atomically with an
initial drain notification. Successful publication, generation replacement and
closure notify after the journal lock is released; rejected publications do not.
Watchers and data readers share the configured subscriber bound and nonwrapping
registration counter. Dropped watches unregister. A collected late notification
may cause a harmless extra drain but carries no old frame or delivery authority.
The lease retains journal state, never the producer/socket, and has no event queue
that a stalled UI or unrelated consumer can fill.

The profile owns BIOS/X-Plane watch leases beside its actual reader. Ownship
watch acquisition reuses the tracker's existing attached or detached passive view,
qualified by exact process identity, enablement and current observation. It never
opens another ownship socket. Registration precedes the initial data read; recovery
reattaches closed leases. Stop drops them. Replacement and launched contexts carry
only the notification target; original reader/capture/command authority is checked
again by the existing native admission and dispatch paths.

Notification-capacity failure is diagnosed while existing adapter service stays
active; it does not turn valid data stale, renew captures or grant a baseline.
Sampled-source acquisition and global process/catalogue/context migration remain
separate required work.

## Verification

- Shared streams: 49 tests pass; all 13 module gates pass. New NativeWake and
  NativeWatch modules have 100% production line coverage; native journal 99.59%.
- Real UDP BIOS commits wake a parked profile reader with a two-second test guard
  and no service tick. Capacity saturation preserves valid native data. Exact
  attached/detached ownship watches reject wrong processes/expired observations,
  and passive watches do not keep sockets open after the producer retires.
- X-Plane's native fixture verifies report and actual expiry notifications without
  renewing original capture stamps; retired readers cannot register another watch.
  This uses a test process monitor, not elevated WMI or an actual simulator.
- Existing profile wake races, source baseline recovery and scripting deadlines
  retain their behavior. Production compilation passes; executable SHA-256:
  eee0e419ab28baee417327d2cd5e26548e3e9dc2068da14b616eddcb8fee7de8.
- The rebuilt actual Cockpit passes authored DCS independent expiry/recovery and
  ownship-only startup, all four Assetto variants, pulse/script completion and
  independent baseline scenarios. Installation lists are restored and cleanup
  errors are empty. Real sinkless physical-input activation/replacement/Stop also
  passes; the controller stayed stationary and no output was authored.

The final full native suite passes 1,924 tests with zero failures and 17 explicit
ignores (95.83 seconds, four workers), with all 129 source-stamped production
module gates passing. Controller: 147 tests and 30 module gates pass. Mapping host:
123 tests and 12 module gates pass. Source notifications have 76.74% coverage;
the telemetry API facade is now 82.61%. No runtime permission was relaxed.

## Evidence and remaining work

Local evidence is under .research/reactive/game-wakes-*coverage/ and the matching
focus/production/native fixture logs. DCS artifact:
.research/dcs-physics-native/1793689473664/evidence.json. Assetto artifact:
.research/assetto-capture-native/1793689530276/evidence.json.

This does not qualify a physical sweep, raw-input-to-display timing, virtual
controller receipt or simulator-confirmed effects. User acceptance stays in #199,
first in the progress lane. The profile virtual-input monitor still has a 500 ms
poll; replacing it with an exact-owner port-sample stream is the next preview task.
Remaining full-app read models, UI/companion/MCP cutover, adapter service and actual
integrated qualification are still open. Estimated overall remaining work: 70%.
