# Windows setup file watch recovery

Startup, library changes and native file notifications now own the
background DCS and Assetto setup baseline, replacing a two-second setup scan.
If registration fails, the affected installation reports that file
notifications are unavailable and withdraws setup verification. A two-second
deadline schedules another registration attempt.
Successful registration inspects the files once and publishes a fresh
baseline. Explicit setup and profile validation retain their own checks.

The setup worker keeps pending work when a setup operation holds the gate.
The gate release or operation completion wakes that work. A pending library
baseline waits for admission before publishing game context. The unavailable
status applies to the affected installation and retains its provider settings
revision across failed retries.

The native watcher installs the event filter with its handles. After
registration it checks the resolved directory set again to catch directories
created while the handles were being armed. Assetto and DCS also watch the
relevant parent paths so removal or replacement of a nested directory wakes
setup.

## Running Cockpit

The authored Assetto fixture first reproduced the missing watch health on the
old executable. An intermediate candidate correctly reported the outage but
missed a nested directory removal. The same mutation passed after the directory registration change.

The final native WebView and browser-dev runs blocked registration with a file
at a required directory path, waited across a failed retry, removed the
blocker, and observed a fresh baseline at the original setup revision. Both
runs observed all four subsequent nested runtime changes and made zero
`game_get_state` or `game_integration_check` requests during observation.
Mutation-to-observation time was 61–63 ms on each transport for this fixture.
These numbers measure setup metadata visibility.

The Windows dev executable SHA-256 was
`47660a2a04c2ba1e75aee5d88e17a8db50576b71728c764e8bfede10de1084f2`.
The browser-dev gateway SHA-256 was
`554bd56da59e90a6a5d84d3080868264d47151983d08cb7e80e7cbd98b0213d3`.

The native DCS fixture blocked the private Saved Games watch, held the
unavailable state across a failed retry, and recovered at the same setup
revision. The full authored regression then passed managed exporter repair,
competing-process withdrawal, profile delivery and independent BIOS and
ownship expiry and recovery. It reported zero cleanup errors. Local evidence
is in `.research/dcs-physics-native/1794107466237/evidence.json`.

The final native Cockpit also saved both values of the process-event setting
and published all 20 expected fallback boundaries to the header and shared
read model with zero state or page requests.

## Local checks

The final Windows native suite passed 2,427 tests with 18 default ignores.
Both pinned archive tests passed separately. An earlier full run hit an
intermittent Assetto preparation proof change. A regression now holds setup
admission and verifies that a deferred library baseline cannot publish from
unchecked setup. The formerly failing test passed the final full run and
15 focused repetitions. The frontend run initially missed one profile
diagram timing threshold under parallel load. The serial run passed all
1,705 tests and Svelte reported zero diagnostics.

The ten changed native production modules reached 75.86–100% production
line coverage using the repository's coverage helper. Declared source
functions reached 83.87–100% after grouping generic instantiations and
excluding test-only code and compiler-generated closures. Raw LLVM function
totals include those additional entries and measured 66.67% for `games.rs`
and 62.32% for `setup_cache.rs`. LLVM provided line and function counts.
The timing audit matched 453 current and 247 historical entries.

The authored tests exercise filesystem and application behavior. Live
simulator and physical controller qualification remain in #199. The
approved shared Windows game discovery fallback and the separate process
proof checks for live readers still apply. CI remains deferred to
integrated qualification.
