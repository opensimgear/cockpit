# Desktop action deadlines reach the native scheduler

The portable desktop scheduler now exposes its earliest pending tap transition,
repeat or moving-pointer tick. An idle pointer cannot retain a stale cadence;
pause, source loss, faults and Stop withdraw deadlines while preserving cleanup.
The query does not advance time, drain delivery or renew authority.

The shared desktop host combines action deadlines with its isolated desktop
reducer's deadlines. The native wrapper forwards that clock unchanged, and the
profile host combines it with canonical reducer deadlines before conservative
conversion to a host Instant. Short authored transitions can therefore shorten
the remaining adapter wait. Parent and worker retain their 5 ms focus/lease
watchdogs. Removing those still requires loss/return notifications that cannot
miss focus discontinuities, independent renewals and child-loss handling.

Local evidence, 2026-11-05:

- All 203 isolated mapping tests and 125 shared-host tests pass, including six
  new scheduler tests and two shared-host deadline tests. They cover 1 ms tap
  dwells and final queue retirement, repeat timing, pointer cadence, read-only
  queries, cancellation/fault withdrawal and isolated shadow deadlines.
- Native integration tests: 10 passed. Full native suite: 2,286 passed, zero
  failures, 17 existing explicit ignores.
- Production line coverage: portable desktop 85.84%, shared desktop 72.70%,
  new shared deadline module 100%, native desktop wrapper 82.42%, native host
  80.63%. All exceed 71%. Native coverage uses the full-suite parent profile,
  excluding its small subprocess fixtures.
- The new opt-in running-Cockpit fixture binds an authored debug input through
  a rising edge to a 1 ms key tap, scoped to the exact process identity of an
  owned receive-only window. One source update produces injected key-down then
  key-up. Stop reports complete cleanup, no held controls and no uncertainty.
- Two native runs observed receiver down/up spans of **47 ms** and **16 ms**
  for the requested 1 ms dwell. These coarse receiver-clock observations do not
  qualify physical latency or 1 ms native timing. They expose remaining desktop
  dispatch latency that stays software work under #193. The synchronous
  ledger/dispatch path needs profiling and further migration.
- The fixture used a separate disposable configuration, then the prior isolated
  Cockpit configuration was restored with the new binary. Receive-only windows
  exited; physical-controller lifecycle/quiet liveness and navigation checks
  pass afterward. The desktop fixture remains opt-in for native injection.
- All 1,153 tracked Rust/manifests/locks match HEAD in the isolated candidate
  except six intended files. Three new Rust files match the candidate. The
  staged native Host excludes the unrelated worktree hunk. The standalone
  mapping-host lockfile also records the existing companion-service streams
  dependency needed by its local test build.

Artifacts are under `.research/reactive/desktop-deadlines-*`; receive-only
evidence is in `desktop-deadlines-native/1793858578883` and
`desktop-deadlines-native/1793858710247` beneath that directory. The final native
executable SHA-256 is
`d6974e18be3b19bb0e832f15a8e51aa27b3a65c6d17a4206d7082aeb3233d3d5`.
Physical controller/preview/raw-to-virtual timing and simulator effects remain
in [#199](https://github.com/opensimgear/cockpit/issues/199). Broader routing and
full-app qualification remain open. CI stays deferred as requested.
