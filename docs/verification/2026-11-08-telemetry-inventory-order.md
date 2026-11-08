# Telemetry inventory ordering under contention

Explicit and background process checks can overlap for the same installation.
The passive telemetry tracker now records the order of an unavailable lookup
even when it retains the existing reader. A late result from an older lookup
cannot renew the process proof, replace the reader or withdraw its capture.
A newer definitive result can still replace it. The unavailable result keeps
the existing process and capture deadlines.

The regression reproduced the defect against `17e92814`. A replacement
observed before the unavailable lookup arrived afterward and advanced the
reader generation from 1 to 2. The regression also checks that an older
matching result cannot extend the original proof.

The combined native regression blocks an Assetto read of authored
shared-memory pages while decoded DCS-BIOS UDP and X-Plane RREF reports
enter their normal capture lanes. The shared tracker receives unavailable process inventory
for all three. The admitted readers retain their exact captures until
their providers publish a new report, and each healthy provider advances
while the other is quiet. Removing the Assetto installation before its
blocked read returns prevents the late sample from publishing. The test
uses fixture process proofs for the native capture lanes.

The rebuilt Windows desktop and browser gateway also ran the authored DCS
fixture. Competing processes withdrew telemetry until only one remained.
Pause, export denial and packet freeze withdrew their affected values.
BIOS expiry left independent ownship live while a subscriber held its
receipt. Restoring the source or catalogue recovered with fresh evidence,
and process exit withdrew both families. Windows event consent and physical
simulator runs remain in #199.

The Windows desktop executable SHA-256 was
`d61daa22b7976ed20d4427fa87389efd03ca1cd16cd92052c064261e8252b2b0`.
The browser gateway SHA-256 was
`ad04a2348e497804ea6dd18c3df0f2d0b067bcd87b69c318f10fc56578ded1e5`.
Both authored DCS runs restored the private installations with zero cleanup
errors.

The combined case passed ten repetitions. The isolated Windows suite
passed 2,475 tests with 18 default ignores. The STM32 deadline test ran
separately, and both pinned DCS archive checks passed. The vehicle
observation module reached 98.11% production line coverage and 100%
function coverage. The native instrumentation emitted no branch counts.
