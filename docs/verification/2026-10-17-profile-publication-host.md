# Native CAN publication and action composition verification

2026-10-17. This verifies the native implementation described in [controller publication](../profile-publication.md). Direct USB and Pico publication overrides remain unsupported.

The native Coordinator/Host fixtures use the production Registry, compiler, DPAPI sealing, panel reader, typed reducer and physical action worker with injected gateway/device providers. No additional GatewayManager, native gateway service, physical discovery or physical writes ran. The gateway identities and device input in these tests are explicit fixtures; this is software evidence, not physical STM32 USB/CAN or game-binding qualification.

The actual Host cases verify durable sealed receipts before claim; exact private-owner inspection; override-to-override, Default and Stop; different logical names for the same physical ports; a real false-to-true input driving its authored setter; action restoration before publication changes CAN routing; and read-only reconciliation after autonomous lease expiry. Independent owners and preserved changes retain an explicit recovery conflict.

Security regressions cover lost replies without claim replay, due-renew route drift, expiry between status and owner inspection, cleanup timeout followed by a release-only explicit retry, stale proofs, malformed sealed targets, all observed USB publishers, unchanged action catalogue identity, and exact post-transition CAN delivery leases. All read pins and delivery tokens come from the real Registry.

Actual Coordinator journal-fault tests fail the refreshed receipt save immediately before Acquire and before Overrides after outgoing cleanup. An undispatched action preparation can be discarded only with its trusted exact receipt or validated durable seal and proof that every driver remains local; attempted Acquire keeps exact receipt matching. Neutralization permanently withdraws publication renewal before fallible action cleanup, while publication release waits for action restoration. Repeated failed Stop does not replay an uncertain action Release. Explicit Recover first verifies storage, then may reconstruct only a finished unresolved worker from its exact private receipt; that worker can inspect and release the old owner without Claim or Invoke. Completed publication restoration proof survives repeated cleanup polling and still rejects independent CAN lease changes.

A composed test exposed a startup defect: the first live panel baseline arrived after reducer/action arming and correctly produced SourceLost. Startup now repeatedly obtains actual live snapshots, admits one exact baseline per source before arming, drops crossed inactive I/O, and preserves only subsequent events. USB/CAN blocking-read and delayed-peer tests retain freshness, cancellation and replay guards.

Local results: 30 publication/Host/security tests, 16 panel-reader tests, 10 action Host tests and 18 Coordinator tests pass. The source-stamped focused coverage gate ran 188 tests across 10 isolated processes; every measured module exceeds 71% production line coverage.

| Module | Covered lines | Coverage |
| --- | ---: | ---: |
| `profile_publication` | 949/997 | 95.19% |
| `profile_session_publication_host` | 160/193 | 82.9% |
| `profile_session_action_host` | 259/292 | 88.7% |
| `profile_session_action_lease` | 880/918 | 95.86% |
| `profile_session_actions` | 552/595 | 92.77% |
| `profile_resource_sources` | 573/623 | 91.97% |
| `profile_resources_game` | 344/357 | 96.36% |
| `profile_session_panels_io` | 193/197 | 97.97% |
| `profile_session_panels_inputs` | 743/772 | 96.24% |
| `game_activation` | 167/168 | 99.4% |
| `profile_session_topology` | 226/255 | 88.63% |

Reproduce the focused gate with `scripts/module-usb-coverage.py` using the modules and test filters recorded in `.research/profile-publication-final-coverage/evidence.json`. Full logs, line annotations and summary are in that directory. Supplementary measurements from the same profile record Session 370/407 (90.91%) and Journal 189/205 (92.2%), excluding executable test-only journal injection blocks; these do not change CI gate scope or thresholds. `.research/p9-companion-live/publication-native-manifest.json` records the exact source and executable hashes. The final native executable SHA-256 is `287ce793772e09334e0a66cbb0e3004eae63cc9a2eb7f2a4ea324e377dd79b78`.

The prior frozen full-suite baseline passed 754 tests, with zero failures and four ignored tests, in 402.36 seconds. The final full native suite passed **758 tests, zero failures and four ignored tests in 403.33 seconds** on the executable above. Its manifest confirms unchanged source hashes and exit code zero at `.research/p9-root-native/final-native-20261017-3`.
