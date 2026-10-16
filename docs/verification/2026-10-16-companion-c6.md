# Companion C6 release preparation — 16 October 2026

The retained release preparation includes the [user guide](../companion-guide.md), [release matrix](../companion-release-matrix.md), portable [examples](../../examples/companion/README.md) and repository release-readiness checker. Desktop installer tooling and its verification records are retired following the browser migration.

The checker validates browser/Android/iOS candidates, resource budgets, pending qualification and eight disabled source gates. A normal check succeeds for the declared blocked state; `--require-release` returns 2. Inconsistent evidence returns 1. The matrix does not authorize releases or replace native runtime gates.

Historical example, mapping, host and service test results remain software evidence only. Signed mobile distribution and upgrades remain #68 with #55/#56; browser/mobile load, display, real game/output and feedback remain #58/#59/#66/#67. No client is release-qualified and feature #37 remains open. Cockpit host output, provisioning, hiding, live-script and mobile-authentication gates remain unchanged.
