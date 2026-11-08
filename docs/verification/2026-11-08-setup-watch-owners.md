# Setup and catalogue watch owners

Each integration owns its native setup watches, aircraft catalogue reads,
and background setup baselines. The shared context observer reconciles
library membership and consumes committed setup state. A blocked setup
read can no longer hold the context observer or another provider's
background admission. Explicit edits and provisioning still take exclusive
setup admission.

Each setup worker waits for its own file notifications or an existing
watch retry deadline. Library changes, committed setup changes and release
of exclusive setup admission also wake workers. DCS and Assetto withdraw
the affected source proof on an observed file change. A fresh inspection
restores it. A stale library scope or a file notification during a read
fences the result.

The setup regression holds one baseline read while another provider
publishes, then retires the held scope and verifies its result is discarded.
The catalogue regression replaces an installation during a read and
checks that the retired result is discarded while a sibling baseline
and the replacement remain available.

The full Windows suite passed 2,467 tests with 18 default skips. The 166
focused native tests and 34 related frontend tests passed. Production line
coverage ranged from 90.59% to 99.00% across the changed modules. The setup
worker reached 99.00% production line coverage. The setup cache reached
20 of 21 named production functions and 62.16% of LLVM's function records,
which also count closures.

The rebuilt Windows dev Cockpit passed the authored DCS fixture with no
cleanup errors. Assetto Rally reported watcher recovery and four file-change
transitions in 59.95–66.52 ms without a state query. Both X-Plane SDK fixtures stayed ready and observed
a catalogue revision after an external write. Executable SHA-256
`21158ae6238058e4f9b17d4692266a3315e5f403d14c00da2f540a1057cdcb65`.

Assetto executable inspection still shares its fingerprint cache. Real
simulator and physical acceptance remain in #199.
