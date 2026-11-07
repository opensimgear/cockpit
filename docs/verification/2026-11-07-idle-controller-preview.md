# Idle controller preview

After the last preview reader leaves, steady HID reports still update the
controller registry and ordered routing journal. They skip the preview's typed
copy and queue. Preparation pins the source until subscription registration
and publishes the latest capture before a new reader receives its baseline.
Metadata and terminal updates keep their existing queue path.

The controller regression covers 50 ordered routing reports while the preview
is closed, matching the reopened baseline to the latest routing cursor and
capture time. The stream regression checks that cancelling one subscriber
preserves demand from other subscribers, including when the cancelled handle
is later dropped.

Both running Windows Cockpit checks passed with an attached VKB controller.
The profile editor showed the assigned device's inputs while the profile was
stopped. The output-free routing fixture matched the preview's controller
generation and value, kept routing live across preview closure and profile
replacement, and stopped and deleted the temporary profile. The live fixture
accepts a routing observation timestamp at or after the matching preview
capture because the routing worker may renew the sample.

The native dev executable SHA-256 was
`8bc0f11da8f59b0648759cba6337dbdd7396f71113889ac60b036cf941c7978e`.
The routing evidence is in
`.research/reactive/profile-source-wakes-native/1794063549888/evidence.json`.
The full native suite passed 2,407 tests with 18 default ignores. Both pinned
DCS archive tests passed separately. The 167 controller tests, 60 native
stream tests and both frontend coverage gates passed. Changed Rust modules
reached 87.17–97.67% production line coverage using the repository counter.
Branch instrumentation was unavailable in the pinned Rust build.

Three alternating runs of the debug `report_cost` example used 137 controls
and 500 measured reports per scenario. Compared with `a0e12bcf`, the median
of the three closed-preview p50 results fell from 276 to 246 microseconds.
Closed-preview p95 varied from 310–460 microseconds on the baseline and
277–421 on the candidate. Open-preview p50 results stayed in the
282–287 microsecond range across both builds. The example measures report
admission cost. Raw runs are in `.research/reactive/controller-idle-capture/`.

Physical motion-to-display, raw input to virtual-controller receipt, and
positive connected-module qualification remain in #199.
