# Routing admission and capture evidence — 2026-11-03

Partial R4 #193 / R2 #191 delivery for parent #188. Physical input-to-preview,
virtual-controller latency and simulator effects remain unqualified in #199.

## Contract

Typed mapping admission now receives an explicit monotonic `admitted_now`.
`Frame.captured_us` remains source evidence for freshness, sequence validation
and script/effect provenance. A delayed fresh input starts its authored pulse or
script deadline when admitted, rather than replaying a historical processing
clock. Retained batch inputs preserve provider order at that admission time.

Future capture evidence and regressing/out-of-range admission are rejected
without advancing the runtime. Original samples are contract-validated before
expiry conversion, so an expired malformed value cannot hide behind `Stale`.
Valid already-expired replacements and source revocations cancel dependent work
before due timers run; independent branches and ordered occurrences survive.
Loss preflight retains its existing logical pre-advance ordering and output
timestamps. This is not an end-to-end transport latency measurement.

The native controller service exposes a read-only copy of its journal's clock
origin. Reading runtime time requires no capture registry lock, report page,
cursor or input evidence. Native deadline conversion, activation and health use
that clock. Quiet-source liveness is checked before advancing mapping time, but
cannot replace a missing/current-generation baseline. Recovery baselines reach
the reducer instead of being blocked by accompanying liveness.

Initial activation baselines use one shared current admission timestamp with
the existing individual-baseline deduplication. A temporary grouped startup
implementation produced an artificial `SourceLost` for unchanged `Unknown` and
correctly triggered physical-output revocation. The physical safety fence stays
strict; existing native publication ownership tests cover the corrected startup.
Live batch loss handling remains unchanged in that respect.

Desktop shadow admission uses the canonical reducer clock. The recording-only
desktop preview explicitly uses its authored replay timeline; it does not infer
real delayed-admission measurements from capture-only fixtures. Existing typed
test fixtures now specify that replay admission explicitly. The mapping-host
standalone lockfile also records its already-declared controller stream dependency;
the affected crate is verified with `--locked`.

## Local verification

- Controller suite: **144 passed**. All module line gates pass; stream and clock
  **100%**. The clock remains readable while another thread holds the registry.
- Isolated mapping: **153 passed**, including eight new clock regressions.
  Typed reducer **92.27%**, runtime **90.41%**, deadlines **100%** and desktop
  **84.75%** production lines; all existing module gates pass.
- Isolated mapping-host: **123 passed** with the repaired lockfile. All module
  gates pass; desktop **72.68%**, dispatch **93.79%**.
- Full isolated native suite: **1,835 passed**, zero failures, 17 explicitly
  ignored. Six focused physical-output publication/ownership tests also pass.
  Changed native production line coverage: source admission **94.12%**, host
  **80.11%**, activation **83.42%**, HID readers **87.46%**, debug input **98.63%**
  and desktop recording preview **90.58%**.
- Final Windows dev Cockpit executable SHA-256:
  `edf0591821370ea4107c01e07727c9816aa40e9f219650b41d928859cc38025a`.
  The authored pulse expires without another input or preview; internal script
  completion/freshness expiry works without a service interval. A supported
  physical integer input routes through an unbound identity output, survives
  same-profile owner replacement, and Stop rejects old-owner inspection. These
  profiles are deleted afterward. No external output is authored by these
  fixtures; no physical sweep or latency measurement is claimed.
  One physical fixture setup initially timed out at the 15-second CDP bound;
  passive observations confirmed no leftover fixture or owner, the native
  inventory responded in 4.1 ms, and the physical recheck passed in 0.87 seconds.
  The timeout's underlying cause is unresolved; this is not a soak/load pass.

Review covered stale sample validation, loss-before-deadline behavior, future
and regressing clocks, delayed pulse/script timing, original source provenance,
baseline occurrence suppression and reconnect baseline/liveness handling. Native
companion metadata recovery exercises the same source-admission helper as health.

## Remaining R4 work

Game profile frames still stamp consumer admission. DCS and X-Plane projections
need to preserve their native per-port capture witnesses through mapping; one
channel's progress must not renew another retained value's evidence. Other
adapter service polling, complete output-stream migration and hardware/simulator
qualification remain open. No physical latency result is inferred from synthetic
clock tests, native acknowledgement or the sinkless running-Cockpit fixtures.
