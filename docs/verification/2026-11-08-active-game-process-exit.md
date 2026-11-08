# Active game process exit

An acquired Windows game reader retains a watch on its exact process. Opening
the handle validates the PID, creation time and executable path. A signalled
process handle wakes the owning profile and withdraws game telemetry, even if
no new capture arrives. The reader also checks for a confirmed exit before
publishing a capture. Closing the reader cancels the watch and invalidates its
evidence without joining the OS waiter from the routing path.

When the shared process observation has a current, unique row for that exact
process, telemetry validation uses the retained exit evidence without reopening
the PID. The shared observation still decides uniqueness. An ambiguous row,
changed event scope, missing exact watch or failed wait keeps the existing
strict or unavailable path. Wait failure uses the validation fallback.
Command and activation admission keep independent strict checks. The existing
validation deadlines still apply to context, setup and owner checks.

The focused Windows run passed 27 tests with one child-only helper ignored.
The native process test waited on a real child and confirmed that its exit
woke the reader. The inventory regression used a real retained process handle
and a controlled process scope. It verified zero repeated liveness queries for
the exact current row, plus independent handling of a wrong creation epoch,
ambiguity, watch retirement and scope retirement. A separate regression kept
the exact watch across owner wake changes and verified Stop invalidates its
evidence. The existing command test still required a fresh strict process lookup.

## Running Cockpit

The rebuilt Windows WebView and Edge browser-dev both passed authored DCS
telemetry with a stand-in process and an independent stalled subscriber.
The fixture exercised competing-process ambiguity, recovery of the sole
remaining process, independent BIOS expiry, pause, export denial and final
process exit. Both clients withdrew active and passive telemetry after exit
and restored their private setup without cleanup errors. This run used the
default Windows process discovery setting. The retained-liveness regression
above covers the injected live inventory scope separately.

Two checksum-pinned DCS-BIOS archive checks and the authored X-Plane 11 and 12
catalogue checks also passed. Assetto and X-Plane live process-event and
simulator checks remain open. User-operated checks are in #199.

The native Windows candidate used the existing isolated `rlib` build with
Cockpit debug info disabled. The committed manifest retains all crate types.
The local source comparison covered 1,883 tracked source and manifest files.
Raw logs are under `.research/reactive/active-exit-watch/`. DCS fixture evidence
is under `.research/dcs-physics-native/1794161048797/` and
`.research/dcs-physics-native/1794161124666/`.

Native executable SHA-256
`fb23a38408ad6100bf4c797f5ffbcadac758b8266d1020e1169459170ae26434`.
Browser-dev gateway SHA-256
`6561cb39fcb2e08a30fc6ca29f490361598275fcc5ffb11f59ccf50c2c256a80`.

## Windows suite and coverage

The final candidate passed 2,472 native tests across the main suite and one
isolated run, with 18 default ignores. The STM32 configuration deadline fixture
failed once in the serial suite, then passed on its own. The follow-up suite
passed the other 2,471 tests and produced a valid coverage profile. The six
measured modules reached 84.33% to 95.20% production line coverage and 71.43%
to 93.33% LLVM function coverage. LLVM emitted no branch counters. The
process-events module change is a type re-export.
