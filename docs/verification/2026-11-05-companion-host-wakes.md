# Companion profile host wakes

The active Windows profile host can now route notification-capable companion,
HID and internal sources to virtual controllers, native desktop outputs and
companion feedback without its blanket 5 ms service interval. Qualification
requires retained host and input wakes, actual acquired native feeds, current
delivery plans and healthy native desktop scope subscriptions where applicable.
Game contexts, vehicle variants, visibility, overrides, physical panel adapters
and unsupported combinations retain their explicit fallback and remain open.

The historical mapping-host `companion_runs.rs` 2 ms worker has no Cockpit caller.
Retired mapping commands are rejected before worker dispatch, covered by the
existing native rejection test. Its former production classification was wrong;
this delivery changes the active profile host instead of that dormant worker.

## Ownership and deadlines

| Work | Trigger or deadline |
| --- | --- |
| Companion registration, baseline, ordered input, heartbeat and loss | Existing SourceSink callback after provider commit/unlock |
| Remaining bounded feed pages | Immediate deadline until every ordered frame/loss is drained; current-state renewal follows ordered transitions |
| Quiet native writer expiry | Exact selected writer `last_seen_us + 1,500,000`; querying cannot renew or expire it |
| Assignment/publication/presence changes | Independent weak routing observer, preserving Activity and inventory observers; lazy initialization binds it after metadata publication |
| Metadata contention or pending feedback clear | A 5 ms retry only after an observed unresolved obligation; held controls awaiting neutral use input/expiry, not a retry loop |
| Rebind capture/compilation | Completion wake, including panic/channel loss; the existing retry remains anchored at attempt + 100 ms |
| Feedback work | Completion wake, including panic/cancellation; existing 34 ms publication limit remains an anchored output deadline |
| Typed processing, desktop releases/renewal and virtual output backlog/renewal | Existing exact host deadlines |

Worker senders disconnect before their completion wake, including unwind. A
canceled blocked job retains its one pending slot until completion; results
retain owner, program, run, generation and revocation fences. Dead or removed
sources settle without an immediate-deadline spin. Unrepresentable native lease
deadlines fail closed and drain one loss instead of spinning at clock exhaustion.

Native baseline capture now rejects unregister/publication replacement at every
checked stage without indexing a removed descriptor. Final acceptance validates
the current subscription, owner, publication and native generation under its
mutex. A valid same-generation edge arriving after the captured neutral snapshot
stays queued and follows that baseline. Wire and native generations remain
independent; wire generation 17 with native generation 1 is explicitly tested.

The production companion clock now reads the immutable controller capture clock
directly. It no longer takes the controller registry mutex or collects a journal
page merely to obtain time. Both APIs use the same monotonic clock origin.

## Verification

The isolated candidate starts at main `28b67005`. The rebuilt running executable
has SHA-256
`d0da1c95d0813c208619e84bf130f3bfc9e2b229de7c7f7bb4cf11adf12df9de`.
All **178 mapping-host tests** and **22 production-module coverage gates** pass;
native feed scheduling has 86.72% line coverage. All **2,320 native tests** pass,
zero failures and 17 existing explicit ignores, with unchanged source/test stamps.
All **ten changed native production-module coverage gates** pass: companion
scheduling and rebind are 100%, feedback 98.31%, pending validation 85.16%,
host scheduling 78.38%, companion adapter 84.89%, companion host 81.38%, native
host 80.75%, companion runtime 91.18% and gateway integration 82.23%.
All 11 coverage-helper tests, Node syntax and 247 pinned timing dispositions pass.

Six actual-Cockpit fixtures pass:

- A real pinned-TLS browser companion supplies input to its compiled profile and
  receives host feedback. Native desktop output is bound only to a separate
  constant false source; no non-neutral desktop input is authored. After Apply,
  1.1 seconds with no gateway reads, and browser reload/neutral recovery, the
  service interval is null, helper authority remains live, held controls are
  empty and uncertainty is false. Profile Stop and listener/settings cleanup pass.
- A delayed controller preview resumes at current absolute state independently
  of another consumer.
- Applied samples reject retired owners and withdraw on Stop.
- Four authored virtual-controller transitions reach the exact Windows Raw Input
  slot; this proves native software delivery, not physical latency.
- An independent remote disconnect still commits one Activity offline event
  through the separate observer, without a measurement-time Flush.
- Neutral desktop profile replacement/Stop retires native scope subscriptions.

Initial checks caught test scaffolding errors: a zero-match filter was rejected
as evidence; the new test's provider clock now advances with admission; a local
binding no longer shadows its token helper; and the live fixture initializes an
omitted empty desktop array. Review also removed an overstrict pending-frame
baseline check and an incorrect comparison of wire and native generations before
final qualification. None of these corrections relax production assertions.

Final cleanup reports audit disabled, no active profile owner, no held or
possible-held controls, cleanup complete, uncertainty false and all virtual slots
free. The running frontend includes preserved user work in progress; native
qualification uses the isolated candidate. The shared host's pre-existing hunk
is excluded from that candidate and the commit, with its original byte image
verified by reversing only this delivery's changes. Other preserved files stay
byte-identical.

This is a scheduler slice, not complete R6 or R9. The existing feedback output
cadence is retained; full per-widget expiry, coalescing/backpressure, publication,
backgrounding, firmware acquisition and combined fault/load matrices remain.
Physical controller/display latency and simulator effects remain unqualified in
#199, first in Progress. The sustained desktop latency workload remains separate
software qualification after the recorded WSL foreground takeover. CI remains
deferred until all feature implementation is complete.
