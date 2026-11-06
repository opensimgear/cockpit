# Held launch scheduling verification

The explicit native launch path wakes the profile worker after arming the
journaled hardware hold, after attempting to finish it, and after cancelling
the exact held owner. The worker schedules the hold expiry at its absolute
90-second deadline. The existing 5 ms service bound still covers adapter
health and lease renewal during the hold.

The gated launch regression verifies that the worker wake epoch changes
before native spawn and again after the profile starts. The coordinator
expiry regression verifies that the deadline shortens the service wait,
disappears after cleanup, and never starts or replays the launch.

The Windows dev Cockpit launched a hidden, output-free native fixture from
idle, recorded a fresh process start, and applied the default profile.
Cleanup stopped the profile and fixture and removed the temporary game.
The attached-controller preview/routing and native workspace checks also
passed. This run did not exercise simulator telemetry or held hardware output.

The local native suite passed 2,357 tests with 18 existing ignores.
Scheduling coverage was 27 of 27 production lines. All three launch wake
sites were exercised. The timing audit verified all 247 pinned dispositions.
