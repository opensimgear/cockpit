# Committed profile read models — 2026-11-03

R5 #194 and R7 #196 are partially implemented; parent #188 remains open.

## Sources and delivery

| Source | Commit hook | Projection |
| --- | --- | --- |
| Saved profiles, machine assignments, virtual slots | Profile service installs a successfully persisted state and cached summaries/local row revisions | `profiles.library` |
| Profile selection and saved session | Same committed service snapshot | Both topics |
| Native owner, background health, activation completion, Stop/recovery/fault | Coordinator captures exact changed typed status | `profiles.runtime` |
| Automatic settings, decisions and preferred profiles | Capture under original automatic-state mutex | `profiles.runtime` |
| Emergency manual suspension | Atomic revision-tagged overlay; no automatic-state lock on Stop | `profiles.runtime` |
| Companion saved-panel reference changes | Eager committed-save hooks and one startup reconciliation | `profiles.library` |

Passive publishing never invokes Gateway `profile_list`, which can reconcile and
persist saved-panel heads. Source callbacks enqueue cached typed projections;
JSON conversion, diffing and broker publication run on a bounded actor. Its
queue is limited by count and bytes, retains a bounded latest recovery state,
and explicitly resynchronizes after loss. Native Stop does not await the actor.
Exact source topics are retired at shutdown. Unchanged projections are suppressed.

Valid large automatic settings retain the existing settings limit. Rule arrays
too large for a single frame become issued SHA-256 manifests; a read-only native
and MCP getter returns UTF-8 chunks. Storage has count/byte limits and pins the
current issued reference. The browser verifies bytes, offsets, digest and count,
deduplicates concurrent resolution and fences retired results. Expired pages
withdraw readiness without repeated reads of the same expired manifest. A later
current complete manifest can restore readiness; wire loss requires a snapshot.

## Consumers and checks

Profile library, editor, runtime controls, automatic controls and game rule
preferences observe shared immutable models. A changed clean row triggers one
pinned document/assignment read; a dirty draft retains its document and history.
Passive updates cannot replay a save notification or complete a pending command.
Stale/read-only state guards Apply; Deactivate remains available during loss and
initial page errors. Unrelated resource/game/workspace and activation-progress
timers remain outside this delivery.

- Isolated native suite: **1,835 passed**, zero failures, 17 explicitly ignored.
  New production line coverage: actor **96.59%**, bounds **100%**, emergency
  overlay **84%**, immutable pages **97.67%**.
- Isolated profiles core: **150 passed**, zero failures. Cached read model and
  virtual slots **100%** production line coverage; service **98.23%**.
- Frontend read-model coverage: **98.48% lines / 88.41% branches / 95.18%
  functions**. Editor and runtime/automatic controllers pass all three 71% gates.
- Svelte check: zero errors/warnings. Vite and native production builds pass.
- Full isolated frontend suite: **1,421 passed**, zero failures with two workers.
  An unbounded run exposed the new page test's event-loop-turn wait and an
  existing graph timing assertion under contention; the page test now waits for
  its actual async completion with a bounded wall-clock deadline.
- Running native Windows Cockpit, executable SHA-256
  `9902c5fbae916987b263631fc64da6a6610d07cb307d077338e1fa7872ff22e1`:
  `tests/ui/reactive-profiles-native.mjs` passes independent native consumers,
  external-client create/rename/delete, one changed-clean-row refresh, dirty
  draft/epoch preservation and no passive save notification. Its temporary
  profile is deleted; no profile is activated and no external outputs authored.
  Evidence: `.research/reactive/profiles-native.json`.
- Native/MCP page parity, expired/unknown digest rejection and read-only getter
  independence from admission are exercised in native tests. The running check
  above uses a second native caller; it does **not** claim MCP HTTP acceptance.

## Open qualification

The 2026-11-06 repeat of the native recording found zero `profile_status` reads
during an idle window spanning two of the former workspace refresh cycles.
System integration inventory now consumes the shared `profiles.runtime` stream.
The regression also checks library and automatic status reads and confirms that
independent consumers still see mutations while preserving dirty authoring.
Whole-app R7, other R5 source families, cross-client MCP application acceptance,
slow-consumer application acceptance and broader cancellation/background-job
acceptance remain open. Hardware/simulator acceptance remains #199 at the top
of the progress lane. This work neither measures nor qualifies physical preview
or raw-input-to-virtual-controller latency.
