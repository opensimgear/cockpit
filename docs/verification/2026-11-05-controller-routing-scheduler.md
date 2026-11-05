# Controller routing without the adapter tick

Qualified Windows/native controller and internal profiles now opt out of the
5 ms adapter service loop, including optional virtual-controller output.
Qualification requires a healthy active reducer, retained native wake handles,
a report watch for every admitted HID source, and supported source and sink
plans. Both source and executable documents are checked. Game/vehicle, desktop,
panel, companion, visibility, publication and physical-action adapters retain
their existing service requirements.

Resource commits have a dedicated live-routing wake independent of temporary
activation, learning and automatic reconnect observers. Committed token answers
are replaced before notification. Read-only validation remains quiet. Native
HID baseline/continuity/liveness ordering is unchanged: drain reports, verify
the current connected baseline, then advance the reducer. Freshness and authored
deadlines still wake a quiet run.

Virtual output drains at most 128 records or 8 ms per poll. A remaining recording
cursor now schedules another immediate turn, preserving ordered edges after the
source burst ends. This does not renew authority. Quiet output retains the
existing send-anchored 50 ms renewal; child exit is checked on new work or that
renewal, within its 250 ms lease. Immediate child-exit notification is not added.

Local evidence, 2026-11-05:

- Full instrumented native suite: 2,286 passed, zero failures, 17 existing
  explicit ignores. Focused routing/input/resource tests: 27 passed.
- A 300-report Session regression exhausts the bounded drain, schedules pending
  work immediately and receives all 300 edges in order without further source
  input. Other regressions cover missing HID watches, pre-baseline report races,
  quiet metadata reads and token revocation before a notified drain.
- Final production line coverage: host 80.85%, scheduler 85.71%, HID adapter
  87.59%, live validation 99.02%, resource observation 95.77%; all exceed 71%.
- The rebuilt running Cockpit passes actual VKB T-Rudder activation, replacement
  and Stop with no preview subscription or external output. Its quiet source
  remains Known after 1,750 ms, beyond the 1,500 ms freshness deadline.
- Authored button transitions reach the actual vJoy Raw Input slot in order;
  Stop leaves the slot free. This qualifies native virtual submission, not a
  physical-controller-to-virtual latency figure.
- Real MCP subscribers pass the same workflow: one stays unread after baseline,
  the other observes all four transitions, and MCP Stop retires both. Listener
  settings are restored. This does not claim TCP-buffer saturation.
- The isolated candidate's 1,151 tracked Rust/manifests/locks differ from HEAD
  only in six intended files; both new scheduling files match it. The staged
  Host source matches the tested candidate while its unrelated worktree hunk
  remains unstaged. Native executable SHA-256:
  `c4393c4fd7d6b21def8d360622dffa31defba502872c683ee2c4264478a0d0e6`.

Logs/coverage are under `.research/reactive/capture-service-*`; physical source
evidence is `.research/reactive/profile-source-wakes-native/1793857472876/evidence.json`.
The initial resource-wake test incorrectly expected a changed epoch to park;
the corrected regression verifies immediate drain instead. All final tests pass.
Physical sweeps, raw-input latency and simulator effects remain in
[#199](https://github.com/opensimgear/cockpit/issues/199). Broader adapter/routing
and integrated qualification remain open. CI remains deferred as requested.
