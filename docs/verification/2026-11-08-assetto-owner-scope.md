# Assetto acquisition lock scope

Each Assetto variant has a separate owner slot. Windows source opening
and reader registration for one variant leave the other slots available.
Within a variant, ordered and latest readers still share the exact owner.
Replacement is admitted after the prior owner's worker finishes.

The process monitor continues to enforce one producer for the legacy
shared-memory namespace used by Corsa, Competizione and Rally. EVO uses
its own namespace. Opening either source still requires that process proof.

The native regression holds a Corsa source open, starts an EVO reader
before releasing it, and then confirms that an ordered EVO reader shares
the existing owner. Releasing the held Corsa open returns its original
source error. The tests use the production slot and reader logic with
authored sources.

The full Windows native suite passed 2,461 tests with 18 default ignores.
The shared source reached 97.24% production line coverage, 75.76% LLVM
function coverage and 92.86% named production function coverage. The
build emitted no branch counts.

The rebuilt Windows Cockpit passed the Assetto Corsa Rally setup watcher
fixture. The watcher recovered after a failed registration and published
four private runtime-file changes without repeat state requests.
Executable SHA-256
`c6382209154e118311c6ec0818794d74e9159bf6e8231663488e9c2b3f812ae6`.
Simulator telemetry and Windows process-monitor consent remain in #199.
