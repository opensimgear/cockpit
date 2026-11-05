# Assetto setup awaits native monitor startup

Assetto setup previously started an asynchronous direct process-monitor probe
and immediately finished with `Preparing native process monitoring.` It now
awaits the committed result through native notifications, cancellation and a
2.5-second startup deadline. Telemetry verification retains its separate
2.5-second window. Passive acquisition remains nonblocking.

The bounded waiter registry retains weak notification leases, carries no
authority, and never requests elevation. A cancelled or timed-out setup leaves
shared monitor authority alone. Exact PID, process epoch and image checks still
guard source attachment. Permission completion alone cannot verify telemetry.
Cancellation during startup returns cancellation rather than a layout proof.
WMI access denial is classified as access denial, preserving its diagnostic.

Local evidence, 2026-11-05:

- Full instrumented native suite: 2,281 passed, zero failures, 17 existing
  explicit ignores. After the final denial-classification regression, the
  focused setup/permission suite passed 48 tests with one existing live ignore.
- Tests cover quiet completion, cancellation wake and isolation from shared
  authority, startup deadline, terminal denial and cancellation during setup
  acquisition. The recorded WMI denial cannot enter telemetry sampling or
  become an unsupported-layout proof.
- Final production line coverage: Assetto setup 75.77%, permission service
  83.21%, new startup waiter 95.12%; all exceed the 71% gate.
- All 1,148 tracked native Rust/manifests/locks match HEAD in the isolated
  candidate except the three intended edited files. The three new source/test
  files match the candidate. Unrelated worktree changes are excluded.
- The final rebuilt running Cockpit passes real VKB T-Rudder shared
  observation/reopen and two complete navigation reader-lifetime cycles.
- The authored four-game Assetto fixture still fails before telemetry
  qualification. It now reaches the actual startup result: WMI denies
  process-event access with `0x80041003`, rather than ending at `Preparing`.
  Temporary installation restoration passed, cleanup errors were empty, and
  no output profile was created. The final artifact is
  `.research/assetto-capture-native/1793856602346/failure.json`.

Logs and coverage are under `.research/reactive/assetto-monitor-*`.
User consent and helper lifetime acceptance for X-Plane and Assetto remain in
[#199](https://github.com/opensimgear/cockpit/issues/199), verified first in the
Plan progress lane. The approved Assetto shared-memory sampler and Windows
process-notification consent are separate requirements. Actual simulator
effects and physical latency remain unqualified. CI remains deferred until
feature implementation is complete, as requested.
