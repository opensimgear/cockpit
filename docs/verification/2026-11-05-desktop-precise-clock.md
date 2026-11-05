# Precise native desktop deadline clock

Desktop parent, helper and receiver now use `QueryInterruptTimePrecise`, converted
from 100 ns units to microseconds. This retains a shared system-wide interrupt
clock and sleep-inclusive lease semantics without changing system timer
resolution or adding a polling/spinning loop. The clock is an independently
covered Windows module; the public `windows::now_us` API is unchanged.

Microsoft documents the old `GetTickCount64` clock's typical 10–16 ms resolution
and the precise API's direct timer-hardware read:
[GetTickCount64](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-gettickcount64),
[QueryInterruptTimePrecise](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-queryinterrupttimeprecise),
[interrupt-time semantics](https://learn.microsoft.com/en-us/windows/win32/sysinfo/interrupt-time).

Local evidence, 2026-11-05:

- The regression failed on the old implementation: its sample was
  `128785000000` us while the surrounding precise samples were
  `128785006038..128785006046` us, approximately 6 ms ahead. The final test checks
  64 native sample windows and observes sub-millisecond values.
- All 84 final desktop-output tests pass. All ten desktop coverage gates pass;
  the new clock module has 100% production line coverage. The six coverage
  parser tests pass.
- The complete native suite passed with the precise implementation: 2,286
  passed, 17 existing explicit ignores. After extracting the unchanged function
  into its separately measured module, all 84 desktop tests and ten focused
  native desktop/scheduling tests pass, and both final app/receiver builds pass.
- The actual running Cockpit delivers a controlled one-update tap, then proves
  complete Stop cleanup with no held controls or uncertainty. The rebuilt
  receiver records 8,729 us between key events. An independent worker QPC audit
  records 9.9087 ms between native call returns. These measure different
  boundaries and neither is a physical input latency measurement.
- Parent audit checkpoint and worker footer are valid/complete, with zero
  dropped records or evidence/I/O errors. The parent wrote all 40 admitted
  records; the worker finished normally after writing all 20.
- Auditing was then disabled. Five final actual-Cockpit receiver, delayed
  physical preview, physical lifecycle/quiet liveness/Stop, shared preview and
  real vJoy/MCP reader/Stop checks pass. Temporary profiles/readers retire,
  receiver/helper processes exit and the vJoy slot returns free.
- All 1,167 tracked Rust/manifests/locks match committed HEAD except the two
  intended edited native files. Both new Rust files and the coverage harness
  match the isolated candidate; unrelated worktree edits remain excluded.

The first exact test filter selected zero tests; the corrected fully qualified
filter reproduced the old failure. The initial audit summary reader expected
footer flags at the top level; it was corrected to validate `footer.report`.
These attempts are not counted as successful validation.

App SHA-256: `a51982b061c3c1e18542df52849c50c6f48b1f21871e0cc0a4a87a6c5e8286e7`.
Receiver SHA-256: `324cc227098ee38a8ece57a79594de2d981b58c23b26ab1bf036ad0aca5d1a13`.
Artifacts are under `.research/reactive/desktop-clock-*`; audited receiver
evidence is `.research/reactive/desktop-deadlines-native/1793868219030/evidence.json`.

Earlier 0/15/16 ms receiver readings used the coarse clock and cannot establish
precise dwell. This change fixes clock quantization, not authored 1 ms delivery:
the final precise observation still exceeds that request. Durable I/O and native
timing remain software work under #193. Replacing parent/helper focus polling
also remains open: event handling must retain away-and-back transitions rather
than inspecting only the latest foreground window. Physical raw-to-preview/
output and simulator acceptance remain #199, first in Progress. CI is deferred
until implementation completion as requested.
