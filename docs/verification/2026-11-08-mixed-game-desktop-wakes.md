# Mixed game and desktop profile scheduling

A running profile with a watched game can park on native notifications when
its explicit desktop scope has its own healthy watch. The existing adapter
gates still require valid source ownership, an active reducer and current
game proof. Selected-game desktop scopes still depend on the separate
activation observer. Child-owned and resuming games, paused vehicle variants
and deferred publication also retain fallback service.

## Running Windows Cockpit

The DCS fixture launched an authored process and sent BIOS and ownship
packets while the profile held a neutral desktop-wide keyboard binding.
The preceding build reported a 5 ms service interval even after the
profile received telemetry and the desktop scope reported healthy.
The candidate reported a null interval both initially and after 1,100 ms
without gateway reads or authored non-neutral desktop input. Its native
desktop helper stayed live with the same authorization. Telemetry still
reached the profile after the interval.

The fixture also passed pause, export denial, frozen and stale ownship,
independent BIOS expiry, fresh ownship startup, respawn and process exit.
Stop released the desktop helper, and cleanup restored the provider
configuration and installation list without errors. These are authored
telemetry and neutral desktop checks. Physical input latency and actual
simulator effects remain in #199.

The first baseline run stopped at fixture setup because the empty
desktop output list was omitted by the profile serializer. The corrected
baseline reproduced the 5 ms interval and cleaned up successfully.

Native executable SHA-256
`a40c8d394fbd8a8933b4d030a5f338c905a7e7a864667794fe9a9f5b2afac0a4`.
The local evidence is
`.research/dcs-physics-native/1794101400025/evidence.json`.

The complete Windows suite passed 2,425 tests with 18 default ignores.
Both pinned DCS archive tests also passed when run explicitly. The
changed scheduling module reached 79.37% production line coverage and
75.00% LLVM function coverage. Both acceptance scripts passed syntax checks.
