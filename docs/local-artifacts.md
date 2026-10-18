# Local research cleanup

Maintained controller mapping probes and their portable tests live in
`tests/controller-mapping-m0/`. CI runs them from that directory. The ESP32
stack-usage diagnostic lives at `scripts/companion-stack-usage.py`; it consumes
the board build's `compile_commands.json` and writes generated reports under
`.research/target-stack/`. The Windows WinMM vJoy reader lives at
`tests/manual/read-vjoy.py`; it reads joystick ID 3 without acquiring a feeder
lease and accepts `--seconds` and `--output`.

Historical migration scripts, frozen source snapshots, patches and investigation
notes recovered on 2026-10-18 are kept locally in `.local-archive/research/`,
preserving their original paths below `.research/`. Its `inventory.json` records
the original path, destination, SHA-256, size, and content matches in current
repositories. These are historical inputs, not maintained tools: scripts may
contain absolute paths, mutate old source layouts, or publish issue updates.
Do not run them as part of normal development.

The archive is ignored because it includes private qualification material and
redundant historical copies. Current production source remains in its existing
repository paths. Live qualification state and compiled browser bundles were
left in place. Generated evidence paths in older verification notes are
historical references; cleanup does not recreate or requalify that evidence.

Uncommitted changes recovered from merged profile worktrees are stored beside
the repositories in `../cleanup-recovery-20261018/`. The USB-info worktree is
retained because its branch contains an unmerged commit.
