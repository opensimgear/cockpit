# Launched process confirmation verification

The explicit launch confirmation wait observes committed process proofs,
admission cancellation, changes to the held profile, and the existing
three-second deadline. It no longer rechecks hold health every 25 ms while
waiting for proof. Proof must match the native process identity and creation
epoch. The original startup discovery and held-adapter health service keep
their independent scheduling.

The observer regression waits for the committed notification, then verifies
that proof is available for the exact process, a different creation epoch is
rejected, and only the producer performs native scans.

The rebuilt Windows dev Cockpit started a hidden, output-free fixture from
idle and applied the default profile after a fresh process start. The
attached-controller preview and routing check and native workspace check
also passed. The fixture covers the actual launch path and does not exercise
simulator telemetry or physical output timing.

The local native suite passed 2,357 tests with 18 existing ignores.
Game activation reached 275 of 279 production lines (98.57%). The new
confirmation wait was exercised. The timing audit verified all 247 pinned
dispositions.
