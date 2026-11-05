# Change-driven companion feedback

The active profile publisher replaces unconditional full snapshots at 34 ms
with one bounded latest projection. The first accepted publication is a
snapshot; subsequent publications contain changed channels or channels whose
own freshness renewal is due. The 34 ms bound remains a maximum publication
rate, not an idle acquisition timer. Fresh channels renew at 750 ms; their
original quality expiry is independent. Unknown, unavailable and stale channels
do not acquire recurring renewal deadlines.

Pure mapping observations expose capture evidence separately from liveness.
A same-value source capture can update feedback; inspecting state cannot invent
a capture. Immutable compiled constant ports remain fresh without pretending
that they received hardware packets. Derived captures identify graph evaluation,
not necessarily a physical device timestamp.

One pending projection replaces older pending state while the independent worker
is blocked. Success acknowledges only the exact submitted channel stamps;
an older completion cannot clear newer pending state. Revocation resets delivery
state. Existing owner, assignment, native/wire generation, cancellation and
250 ms maximum job-age fences remain in force. Serialization and delivery stay
outside mapping admission. No manual/MCP command or wire schema is added.

## Local evidence

The isolated candidate starts at `300e687f`. Production executable SHA-256:
`729d8f9f4b956c092f0b09f7ed343fe412ece4d5f789556df0340c561663618f`.
The staged native tree matches all 1,241 candidate records; all 1,228 native
coverage source/test stamps remain unchanged. All 107 recorded unrelated
workspace file hashes are preserved, including the existing shared-host hunk.

- 208 mapping tests pass; all 36 production coverage gates pass, including
  21/21 executable lines in the new observation module.
- 2,332 native tests pass, zero failures, 17 existing explicit ignores.
  Changed gates: publisher 141/144 (97.92%), delivery 52/52 (100%), scheduling
  31/31 (100%), companion source owner 872/1027 (84.91%).
- 26 native companion-service feedback tests pass, including independent ages,
  bounded latest state, session isolation and native transport resynchronization.
- All 178 mapping-host consumer tests pass against the new observation API.
- Firmware core/raster harness and six network harness binaries pass under
  AddressSanitizer/UndefinedBehaviorSanitizer. All configured coverage gates
  pass. The core has 100% production lines. A two-channel snapshot followed by
  a speed-only delta preserves caution's receipt time; only caution expires
  and requests widget damage at its own boundary. Wire and serial cases reject
  retired generations and prevent duplicate/older sequence replay. These are
  software harnesses, not physical ESP32 qualification.
- Browser feedback: 5 tests and 100% coverage; companion UI: 34 tests with
  97.91% lines, 96.74% branches and 96.30% functions.
- Eleven coverage-helper tests and all 247 pinned timing dispositions pass.
- Production native build and Node fixture syntax pass.

The real pinned-TLS browser fixture rejects the previous build: it publishes
24 times in 1,116 ms. On this build it records four publications in 1,111 ms,
with independently fresh cached feedback. This measures publication frequency,
not physical input/display latency or CPU percentiles. Apply, browser input,
feedback, reload/neutral recovery and Stop pass. The native service interval
stays null; helper authority stays live and held controls stay empty.

Running-Cockpit latest-controller recovery, profile samples/owner retirement,
exact vJoy Raw Input output, Activity committed-source wakes and desktop focus
lifecycle also pass. Final cleanup reports a stopped profile, no held or
possibly-held desktop controls, no uncertain ownership and free virtual slots.

## Corrections and remaining qualification

Early new mapping tests assumed an absolute revision, millisecond expiry in a
microsecond API, and no capture marker for a compiled constant. They were
corrected to the actual contracts. New native fixtures initially mixed heartbeat
and input counters and confused feedback's one-second quality boundary with the
native source's 1.5-second lease; final full suites pass the corrected tests.
Two early incorrect test filters matched zero tests and are not evidence.
The coverage-helper suite needed an accessible workspace temporary directory.
The new two-channel firmware fixture initially changed its baseline before old
mutation assertions and left a channel in its formerly empty snapshot. Those
fixture errors were corrected without changing firmware production code.
A subsequent assertion exposed that its speed-only delta still contained the
second channel; removing that unchanged value makes this a true partial delta.

Physical companion rendering, actual simulator feedback, complete reconnect and
backgrounding matrices, sustained mixed-consumer load, and full #188 acceptance
remain open. Controller/display latency and physical rigs remain in #199.
The sustained desktop timing interruption by `msrdc.exe` remains separately
unqualified; its launch origin is not established. CI remains deferred until
all feature implementation is complete. No original broad acceptance checkbox
is closed by this bounded delivery alone.
