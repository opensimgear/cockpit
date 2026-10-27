# Activity history

Activity is the persisted native history of meaningful changes in Cockpit. It is
collected while other pages are open and includes operations made through MCP.
Repeated status reads, control input samples and telemetry values do not create
events.

Automatic remembered-device cache saves are bookkeeping and do not appear in
Activity. Existing cache-save rows are also excluded from search, counts and
filters without deleting stored history. Explicit device Forget actions and
failures to save remembered devices remain visible.

The native collector records game library changes, game process and integration
context changes, saved profile edits, activation and runtime health transitions,
device and companion availability, configuration operations, and failed
operations. Edits show the saved before-and-after values, including profile
nodes, settings, connections, required resources, assignments, game tags and
installation paths. Degraded profiles and failed operations show the actual
reason in the row and its details. A save that fails to apply is recorded as a
warning with both the saved changes and the application error. Each
event has a UTC timestamp, type, category, severity, source and related entity
identity when available. Credentials and configuration bodies are excluded.

The compact list shows the newest events first. Expand a row for its full
message, all change details, exact timestamp and entity ID. The compact row
previews its first change; hovering shows the full text. Secret values and
script contents are omitted while useful diagnostic explanations remain.
Search uses SQLite
full-text matching across messages, details, names, IDs, types and categories;
multiple search terms must all match. Search text is treated literally rather
than as SQL or full-text query syntax.

The **Filters** button opens time, category, severity, type and exact entity-ID
filters. Values within a group match any selected value; groups combine. Custom
times use the computer's local timezone. Older pages retain an event boundary so
new incoming events cannot shift the rows being read. Return to the newest page
to resume automatic refresh.

**Settings → General → Activity** controls retention in whole days, from 1 to
3650. The default is **7 days**. Expired rows and search entries are removed on
reads and writes, on retention changes, and by periodic native cleanup.
Shortening retention deletes older events immediately. **Clear all** asks for
confirmation and deletes every retained event, including rows outside the
current filters; subsequent activity continues to be recorded.

The SQLite database stores the event history, full-text index and retention
setting. Opening an existing current database installs the Activity tables
without resetting other settings. Native event writes run on a dedicated worker,
in bounded batches; failed writes stay pending for retry and log reads surface
the persistence error instead of claiming the history is complete.

An action has one persisted event. Its acknowledged changes consume matching
background observations, including observations captured before the command
finishes. Independent health changes and deliberate repeated actions remain
separate. Each queued event has a stable identity; replaying a database write
cannot add another history or search row. Provider-backed Forget commands own
their event, while local-only inventory Forget remains logged. These rules
apply to newly captured activity; earlier history is preserved.

MCP exposes `activity_list`, `activity_settings_get`, `activity_settings_save`
and `activity_clear`. The two mutations require write access and an
`operationId`, following the same replay rules as other Cockpit mutations.
`activity_list` accepts `search`, `categories`, `levels`, `eventTypes`,
`entityId`, UTC millisecond `since`/`until`, `offset`, `limit` and `throughId`.
Retain the returned `throughId` when paging a captured result set.

Run `pnpm activity:coverage` for the frontend model. Native Activity modules are
included in `scripts/integration-coverage.py`, and SQLite Activity modules in
`scripts/persistence-coverage.py`. `pnpm activity:native:ui` requires an actual
desktop Cockpit, `OSG_ACTIVITY_NATIVE=1` and an explicit
`OSG_ACTIVITY_TEST_DATABASE` inside `.research/activity-native`. The test checks
the running native database path before changing or clearing generated test data.
