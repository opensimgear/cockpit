# Companion generation recovery

A deterministic native Host regression captures a genuine Hub/Registry
observation, reconnects the same authorized peer, obtains its new neutral
baseline, and then publishes the earlier observation. Before this fix Apply
failed on the crossed generation despite having time left to acquire fresh
data. The regression uses actual private assignment and Registry tokens.

Acquisition now validates all owned instances and captured source identities
before converting any baseline. A crossed generation remains Not ready while
the existing Host loop refreshes and binds current tokens. Its three-second
deadline, cancellation checks, exact contracts and assignment ownership remain
unchanged. Live conversion still rejects a mismatched generation.

A second regression exposed a dropped loss notification: converting a Gap
cleared the reader's ready flag, then a later generation error discarded the
whole batch, including its buffered Disconnect. The error path now remembers
whether the source was ready before processing. It discards failed-batch input,
emits one fresh Disconnect and clears only owned feedback. The actual typed
reducer immediately marks source and destination unavailable with SourceLost;
sequence numbers keep increasing and held input is not replayed.

Local verification on 2026-10-17:

- 30 companion/Host/launch tests pass, including both regressions.
- Affected line coverage passes the unchanged 71% gates: companion sources
  775/859 (90.22%), companion Host 199/246 (80.89%).
- The complete parallel instrumented native test phase passes 760 tests,
  with zero failures and four prerequisite-dependent ignores, in 182.44 seconds.
  All 362 native and two runner source hashes stay unchanged during the run.

Evidence is retained under `.research/p9-ci-native/`, including
`companion-final-validation.json`, `companion-generation-coverage/` and
`action-coverage-final/native-tests.log`. The post-Cargo test executable is
copied as `companion-final-tested-native-tests.exe` with SHA-256
`0618596dff4473526da2b5b0263abc260f6da76ef3a6f842196866fecd560803`.

The complete action-coverage runner's report phase is inapplicable on Windows:
the Unix-only local gateway has no compiled production lines. Its native test
phase passes; the three compiled action modules measure 97.60%, 93.95% and
85.98%. Linux CI retains its existing four-module gate without changes.

These are controlled software/peer checks. They do not establish simulator
telemetry, physical controller/driver behavior or real-rig qualification.
