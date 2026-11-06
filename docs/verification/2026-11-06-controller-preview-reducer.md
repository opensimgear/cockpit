# Controller preview reducer timing

The preview reducer reuses the control index across same-order snapshots.
It matches repeated descriptors against the validated baseline and preserves
unchanged rows. Sparse changes validate and update only the supplied values.
A regression verifies that sparse values still reach the correct control
after an absolute snapshot reorders the rows.

A synthetic comparison with `6a65b647` used two listeners, 137 controls,
4,000 measured frames per case and a 500-frame warmup. With one changing axis
in each absolute snapshot, median reduction time decreased from 43.9 to
37.3 microseconds and p95 from 57.1 to 52.5 microseconds in the second run.
For a sparse single-axis change, the median decreased from 7.1 to 4.0
microseconds. These figures measure JavaScript reduction only.

The running Cockpit with an attached controller passed delayed preview
receipt, exact capture sharing, preview reopen, and output-free profile
routing across preview closure and profile replacement. Svelte checking
reported zero diagnostics. All 16 reducer and 27 transport tests passed.
Reducer coverage was 99.61% lines, 96.53% branches and 100% functions.

The broader inventory suite passed 114 of 130 tests. The same 16 USB setup
tests failed on the preceding isolated baseline. The follow-up fixed the
test harness, which failed to mount the metadata observer when the component
used Windows line endings. All 21 USB detail cases now pass with LF and CRLF.
The suite also asserts that navigation releases both fixture readers.

The full 130-test system inventory suite and the inventory and USB coverage
gates passed. The running native Cockpit passed its navigation trace and
reported zero USB setup endpoints. Physical USB ownership and controller
motion-to-display and virtual-controller timing remain in #199.

## Frame scheduling

Both controller inspectors apply the newest continuous report at the next
display frame. The initial baseline, replacement and loss are immediate.
Closing the view cancels its pending frame. Raw observers and native routing
keep their independent delivery paths. The fake frame-clock regression
checks coalescing, immediate loss, baseline recovery and cleanup.

The visible native Cockpit rendered all 125 matched reports in a ten-second
passive VKB Gladiator trace. The estimated journal-to-DOM median was 11.9 ms
and p95 was 13.6 ms, with 2.25 ms clock alignment uncertainty. The p95 to
the following measured frame opportunity was 20.1 ms. This was a passive
software trace without a controlled sweep, baseline comparison, physical
display measurement or virtual output.

The running Cockpit passed delayed preview receipt and output-free routing.
All 17 controller observer tests and 130 inventory tests passed. Observer
coverage was 98.68% lines, 96.74% branches and 97.37% functions, with zero
Svelte diagnostics on the isolated candidate.
