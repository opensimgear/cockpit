The parity guard runs with `node scripts/mcp-parity/check.mjs`. Its regression
tests run with `node --test scripts/mcp-parity/check.test.mjs`. Both require only
Node, with no running cockpit or installed dependencies.

The inventory files contain the workflow-level semantic audit. Each row must
provide workflow, ui, native, mcp, status, input, output, effect, guard, lifecycle,
and scenario strings. Source paths mentioned in the source/scenario columns
must still exist. Scenario prose describes the required acceptance behavior;
it is not automatically counted as an executable test.
Status must be `supported`, `partial`, or `missing`. All `inventory-*.json`
shards are loaded; duplicate workflow identifiers fail.
`workflows.json` pins reviewed workflow identities across shard moves/splits.
Removing an inventory row therefore fails even if its entire shard is removed.
Every shard must contain rows. A new workflow needs a linked tested contract;
retiring an old workflow requires explicitly reviewing the workflow manifest.

The guard derives existing exact action anchors directly from committed
cockpit HEAD `3f4dbe696efbd00129f0b4acef3d5fddce5af47f`, using Git's tree and
batched blob reads. This baseline keeps the
current audit usable while recording existing parity gaps. It is not evidence
that every existing action already has MCP behavioral coverage. There are no
generated action index shards to maintain or append to. Use contracts for new
actions. Full Git history is required, as configured in CI. Removing an old
unregistered source action is allowed; retiring an explicitly registered action
fails its contract until that registration is reviewed and removed.
`catalogue.json` similarly indexes
the existing discoverable MCP tool names and rejects removed names.

For a new action, add a workflow inventory row and a `contracts.json` entry:

```json
{
  "workflow": "Exact inventory workflow string",
  "tools": ["discoverable_tool_name"],
  "tests": [{"path": "tests/ui/feature.mjs", "anchor": "exact test title"}],
  "actions": [{"path": "src/lib/Feature.svelte", "kind": "ui", "anchor": "onclick={save}"}]
}
```

Use the exact action reported by the failing checker. The test must exist,
contain the exact stable title/anchor, and contain an assertion. Review must
confirm it exercises the MCP entry point and checks the workflow's observable
result, guards, and lifecycle; lexical discovery cannot prove test semantics.
A missing tool can instead be registered explicitly with `proposedTools`:
each proposal must contain nonempty name, input, output, effect, guard, and
lifecycle strings. It requires a `partial` or `missing` inventory status and
the same behavioral test registration (for example, the current negative
scenario). This registers an acknowledged gap; it does not claim parity.
Once the tool exists, move it to `tools` and update the scenario to verify it.
Five existing representative registrations connect listener revision handling,
widget source replacement, publication upload, prepared profile authoring, and
theme persistence to their actual native/UI behavioral tests. Their exact
source actions and test-title/function anchors are checked on every guard run.

Discovery covers legacy `on:event` and modern DOM event handlers in Svelte,
exported asynchronous TS action wrappers and literal invoke/request/command/
mutate calls in src/lib, native Tauri command functions, and named gateway
dispatch match arms. Literal `command: 'name'` properties in TS/Svelte are
independently discovered, including new calls inside an existing handler body.
A modified action expression becomes a new anchor, while
presentation changes do not. Existing named handler bodies can change without
changing their anchor: reviewers must consider behavior changes as well as new
entry points. New dynamic dispatch forms or event syntaxes require extending
the discovery function and its tests. This is an explicit review guard, not a
complete parser or a substitute for the semantic audit and live scenarios.
The dedicated MCP workflow parity CI job always runs the guard and its
coverage-qualified tests on pushes and pull requests, independently of Nx
affected-project selection. Source traversal is limited to application,
native, package, crate, script, and test roots and skips hidden/build/vendor
directories; arbitrary local logs, research folders, and JSON are not read.
