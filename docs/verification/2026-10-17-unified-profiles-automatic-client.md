# Automatic profile selection client — 2026-10-17

This frontend foundation consumes native automatic-selection status and exposes
only explicit mode/rule edits. It does not scan processes, submit process
identity, apply a graph, or turn editor navigation into runtime selection.

The client accepts native schema version 1 with Manual/Automatic mode, manual
profile pin, at most four distinct game rules, status/reason/conflicts and
read-only current/last-attempt targets. Each rule references a canonical saved
profile, a native validated executable identity, an enabled flag and signed
16-bit priority. Browser writes send only `gameId`, `profileId`, `priority` and
`enabled`; executable, PID, process-session, graph and Apply fields are absent.
Native process-session strings in status remain read-only metadata.

The supported requests are `profile_automatic_status`,
`profile_game_rule_save`, `profile_game_rule_remove`, and the existing explicit
`profile_selection_mode`. Saved choices come from unified `profile_list`, with
strict bounded profile-summary validation including the protected Default.
Removing a stale rule remains possible without reopening its game installation.

| Validation | Result |
| --- | --- |
| Typed client | 6 tests; 83.33% lines, 98.13% branches, 100% functions |
| Fenced controller | 4 tests; 100% lines, 89.36% branches, 100% functions |
| Browser/native transport suite | 188 passed |
| Main and browser Svelte checks | 0 errors, 0 warnings |

Both new helpers have independent 71% line, branch and function gates in CI.
Tests cover immutable captures, exact request bounds, malformed/unknown identity
fields, stale or missing native replies, exact write confirmation, no retries,
read-only admission and late-response fencing. Automatic status has a separate
read queue; rule and profile writes keep storage ordering. Policy changes cancel
older queued input-learning starts. No read or controller disposal starts or
stops output.

These checks cover the client/controller unit. Game-details rule authoring,
Profiles mode controls and native automatic reconciliation have separate
integration acceptance.
