# Native Windows process regression

From the repository root, run `python scripts/windows-process-smoke.py` on Windows
with the pinned Rust toolchain installed. Cargo uses the fixture lockfile and
offline dependencies. Each invocation creates a new `.research` evidence directory
and an independent Cargo target, then copies both executables before running them.
Use `--output-dir PATH` to select a new evidence directory.

The fixture includes the actual native discovery and process Observer source
modules by path. It creates bounded GUI subsystem helper processes named `DCS.exe`
and `X-Plane.exe` in private installations; it never starts a real game, Manager,
provider, GamesRuntime, or device. Each helper exits after at most 15 seconds and
accepts stop/ready files only in its private run directory. Cleanup uses the owned
Child handle and stopfile, including on an assertion failure.

Assertions cover real Windows enumeration, image/creation epoch checks, external
start/exit, two-observation debounce, stale proof rejection, sequential same-path
replacement, unrelated installations with the same basename, simultaneous matches
at the exact same configured path, simultaneous DCS `bin`/`bin-mt` matches, and
X-Plane's global exclusivity across different paths. Ambiguity must clear Observer
authority; the remaining single process needs two fresh observations.

The evidence directory records logs, result details, binary hashes and matching
before/after production and fixture source hashes. These checks establish native
Windows process behavior with owned fixtures. They do not qualify real-game
launch, GamesRuntime/Coordinator composition, simulator telemetry, or hardware.
