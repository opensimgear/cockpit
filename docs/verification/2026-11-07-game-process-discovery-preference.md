# Windows game process event preference

General Settings offers an opt-in to Windows process start and exit events.
The saved default is off. The existing WMI watcher uses the current
Windows account's access. If the setting is off or the subscription is
unavailable, one native owner captures the registered game process inventory
every second. Activation and integration context project their own rows from
that same capture. The persistent Assetto Corsa menu can count for activation
while remaining outside the telemetry process selection. With no eligible
games the worker waits for a catalogue change.

The `game-process-events` preference is available through the same revisioned
UI and MCP read, save and observation operations. The user approved the
one-second fallback in [#189](https://github.com/opensimgear/cockpit/issues/189#issuecomment-6712663423).
Command-time and applied-session process verification retain their shorter
safety budgets.

## Running Windows Cockpit

The native WebView and browser-dev each saved both switch values through
General Settings. With events off, each ignored an authored executable with
the right name at the wrong path, published all 20 expected boundaries to
the Active game header and the shared read model, and withdrew the old
identity when its registered executable was replaced. Each run made zero
`game_get_state` or `game_read_model_page` requests during the boundaries.

| Transport | Authored start to header | Authored exit to header |
| --- | ---: | ---: |
| Native WebView | 1.81–1.96 s | 0.74–0.83 s |
| Browser-dev | 1.77–1.96 s | 0.74–0.82 s |

The start measurement includes the two independent matching observations
required for automatic activation under fallback. The observed numbers
describe game discovery, separate from input routing and presentation.
The Windows account's event access remains unqualified under #199.

## Running Cockpit with the switch enabled and access denied

The native WMI registration probe returned `0x80041003` under the current
Windows account. The live fixture then enabled **Use Windows process events**
in each client's General Settings and confirmed the saved value stayed on
throughout the process checks. Both clients ignored the same-name executable
at the wrong path, showed all 20 expected transitions in the header and
shared read model, and made zero repeat game state or game page queries.

| Transport | Authored start to header | Authored exit to header |
| --- | ---: | ---: |
| Native WebView | 1.77–1.93 s | 0.74–0.86 s |
| Browser-dev | 1.77–1.97 s | 0.72–0.85 s |

Both clients used private databases. The native executable SHA-256 was
`65828f09e640edcc5ad3aed68481bafc27783323db29ed76d5d47a9c393a7812`.
The browser-dev gateway SHA-256 was
`cd73a4ce68c7ba402b60ae3513072a4216f5f09fb38cfbee1b02b1f54edd808b`.
The focused activation observer test and nine process-event software tests
passed. Granted Windows access and live watcher lifetime remain in #199.

## Related authored DCS verification

The native authored DCS fixture passed with the same executable. It verified
shared discovery of the fixture process, ambiguous-process withdrawal and
recovery, DCS-BIOS and ownship delivery into a profile, and final process-exit
withdrawal. Its private settings and installation were restored without errors.
The raw local record is `.research/dcs-physics-native/1794084582052/evidence.json`.

Native dev executable SHA-256
`6a28fa6ad8d7d4ac0705341105267b5a7f6d4d753dd12c768520798e68464ff6`.
Browser-dev used the directly linked gateway because an existing process
held the top-level copy. Gateway SHA-256
`ab7b8f2e763081b7408b3ed5facbd95b330de2e2370f7a795bb6a813fd8fecb6`.


## Local suites and coverage

The Windows native suite accounted for all 2,443 named tests across the
initial and resumed runs. There were 2,425 passes and 18 default ignores.
Two of those ignored DCS archive tests passed separately against the pinned
archive. The initial process ended before emitting a complete coverage
profile, so the remaining tests were resumed and 355 completed tests from
the affected modules were rerun in four batches for coverage.

All 13 changed native production modules passed the 71% line threshold.
Their results ranged from 79.93% to 100%, including 95.81% for activation
runtime, 85.66% for shared discovery and 100% for the context process cache.
The frontend suite passed 1,708 tests and Svelte reported zero errors or
warnings. The configured UI preference coverage reached 100% lines,
97.50% branches and 100% functions. The timing audit accounted for all
452 current sites and 247 baseline sites. CI is deferred to the integrated
feature check.
