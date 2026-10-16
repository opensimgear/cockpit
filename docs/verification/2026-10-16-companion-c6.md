# Companion C6 release preparation verification - 16 October 2026

Issue: [#44](https://github.com/opensimgear/cockpit/issues/44).
Release qualification: [#68](https://github.com/opensimgear/cockpit/issues/68),
with existing #55-#62/#66/#67/#58. Feature #37 remains open.

## Delivered boundary

C6 publishes the [user guide](../companion-guide.md), a truthful
[candidate/release matrix](../companion-release-matrix.md) and portable
[flight/button-box and axis/racing examples](../../examples/companion/README.md).
The examples contain complete logical bindings and feedback widgets, without
credentials, assets, machine paths or local virtual-device assignments. Import,
save, publish and recording preview do not start game output.

The Windows/Linux/macOS package verifier now produces schema 2 development
evidence: exact application version/identity, native target/architecture, artifact
hashes, config/locks, source/build-input digest and protocol/trust contract digest.
It validates NSIS PE/bootstrap metadata, Debian version/architecture and macOS
plist/Mach-O/native architecture plus bounded archive paths/duplicates. NSIS
payload/install/signature verification remains qualification, not a PE-header
claim. Source archives explicitly have no authenticated Git commit identity.
Changes to build inputs during packaging are rejected. Compatibility comparisons
verify both artifact sets and report metadata only; no trust migration, native
install/update or release result is inferred. Legacy schema 1 evidence remains
verify-only. No automatic updater, signing or publishing is enabled.

The release-readiness tool validates all five candidate clients, C0 budgets,
pending qualification owners and eight actual disabled source gates. A normal
check succeeds on the declared blocked state; `--require-release` returns 2.
Malformed/inconsistent input returns 1. No target can become release-qualified by
editing this development ledger. Source-pattern checks are repository consistency
guards; they do not replace native runtime gates or qualification.

The C5 Windows CI failure was a test unconditionally unwrapping a deliberately
nonblocking submission-inspection result while the route worker held its lock.
The test now waits only on the documented busy result with a fixed deadline,
checks other errors immediately and distinguishes independent lease Renew from
output mutation. A deterministic lock-held regression verifies prompt busy
response with no output. Production try-lock isolation is unchanged. The previous
macOS startup issue passed in C5's remote job; it is not inferred from Windows.

## Local validation

Checks use clean `7d41100` source archives plus only C6 changes, in isolated
Windows and Linux workspaces. Other chats' uncommitted controller, desktop,
game/telemetry work is excluded and preserved. An independent ESP32 commit
`b558749` advanced the shared branch during validation. Its 55 paths are disjoint
from C6; both new service integration tests passed against the combined tree,
and that commit is preserved. Development artifact provenance remains the exact
archived build-input digest recorded below, not a claim of a signed final-commit build. No controller driver,
HidHide policy, real game, signed installation or protected-store lifecycle was
changed for this work.

- Full root frontend CI passed: Svelte/TypeScript, transport/regressions, all
  existing frontend coverage commands and production build. Companion contract,
  designer, renderer, cache/lifecycle, host/helper suites and production build
  passed, with example coverage now included in `companion:test`.
- Public examples: eight meaningful import/save/reopen/export/publication,
  simultaneous control/fallback/cancellation and typed feedback isolation tests
  passed. Generator coverage: 97.50% lines, 95.24% branches, 100% functions.
- Package tool: 15 tests passed, including mocked native orchestration and
  identity/provenance/target/artifact/compatibility failures. Release checker:
  seven tests passed, including false release claims, source-gate changes,
  removed follow-ups, changed budgets and CLI exit 0/1/2. Standard-library trace
  measured executable source-line coverage: package 94.96% (358/377), release
  checker 98.98% (97/98). These are line measurements, not branch measurements.
- Mapping host: 67 tests passed on Windows and Linux. All eight Windows native
  coverage gates passed; companion routes 90.01%, router 87.42%, service 75.69%,
  companion source 97.70%. The fixed/new inspection tests passed 25 repeated runs
  (50 test executions) without weakening worker isolation.
- Native Windows host: 364 tests passed, one official DCS archive test ignored;
  companion service: 63 tests passed.
- Ordinary mapping acceptance, C4 three-peer isolated recording acceptance and
  C5 two-peer native feedback acceptance all passed again against the unchanged
  C5 private-service production executable. Twelve Python acceptance-helper
  tests passed. These are loopback fixtures, not physical qualification.
- Actual Windows development packaging passed native companion 5-test lifecycle
  suite, frontend build and signing-disabled locked NSIS build. The 3,987,082-byte
  installer SHA-256 is
  `b3d20b02b2c1d8a54480a0c487cbb874d53243d119b84d656082050c6a3c2d40`.
  Schema 2 artifact verification and self metadata comparison passed. The archive
  build-input digest is
  `0de560c6a7059276d3169ecf57613ae39b90c9aede4442d09e0cbbeff0f168bd`;
  `source.commit` is null, not an inherited enclosing-checkout commit claim.
- Actual Linux development packaging passed native companion five-test lifecycle
  suite, frontend checks/tests and a signing-disabled locked amd64 Debian build.
  The 64,391,340-byte package SHA-256 is
  `397ee97432e32b33241fd78844131ba88220a3b04517a2e05035a0aa8b1e6e17`.
  Schema 2 verification and self metadata comparison passed, with the same
  162-input source digest as Windows. No install/sign/update result is claimed.
- macOS development package/native compilation passed C5 remote package CI
  38879833239. C6's stricter native macOS verifier runs on the macOS CI matrix
  after push; it cannot be executed as native evidence on this Windows machine.
  iOS/Xcode/device and all signing/distribution checks remain unqualified.

## Acceptance split

#44's release-preparation software criteria are completed by this delivery.
Original signed/native/game/performance criteria remain unchecked in the pending
stories recorded in the matrix. #68 adds explicit compatible/incompatible version,
trust rotation, signed-install interruption and rollback qualification, reusing
platform evidence in #55-#62. #58 retains mixed-peer performance, #66 selected-host
game/output and #67 real feedback/captured-assignment recovery.

No client target is release-qualified. #37 remains In Progress. Windows is the
selected host qualification target; Linux/macOS companion support does not enable
the deferred #48/#49 host backends. Unqualified output, provisioning, hiding,
script, mobile authentication and Linux isolated-launch gates remain disabled.