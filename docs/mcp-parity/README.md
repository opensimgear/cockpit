# Cockpit workflow and MCP parity

Use this inventory when adding a portal control, implementing a typed MCP adapter, or deciding what the
embedded assistant can report to its user. The audit starts at committed `3f4dbe69`. A row describes
an observable workflow, its UI entry point and native owner, current tools, inputs, results, effects,
guards, lifecycle and acceptance scenario. Several buttons can share a workflow when they use the same
document edit, native admission and readback.

| Workflow area | Inventory | Implementation contract |
| --- | --- | --- |
| Profiles, working copies, graph editing, learning, game library and integrations, devices, calibration, debug and installers | [Core workflows](inventory-core.json) | [Editor, game, device and installer proposals](core-contracts.md) |
| Companion identity, listener, designer, publication, firmware, local preferences, activity, backups, MCP administration and planned AI controls | [Companion, settings and assistant workflows](inventory-other.json) | [Authoring, provider, settings and chat proposals](other-contracts.md) |
| Cross-cutting authority, approval, providers and progress | [Shared native contracts](execution.md) | [Development and CI check](GUARD.md) |

**Supported** means the current typed path can produce the workflow's specified outcome, subject to the
same platform and device restrictions. **Partial** identifies a difference in inputs, state visible to
the editor, effects or lifecycle. **Missing** means the corresponding typed operation is absent. Planned
AI controls and the two native profile operations without a confirmed visible control are labelled in
their rows. They count as design obligations and must be distinguished from existing portal controls.
The scenarios specify what to observe. See the [live verification record](verification.md) for executed checks.

## Where behavior differs today

* In Profiles, New and Copy create an unsaved working copy. MCP create and duplicate immediately persist
  a library entry. The UI's Save can replace its own active run after committing the document. MCP
  prepared save persists the graph and assignments while the existing applied revision keeps running.
  Preserve both outcomes, including a successful save followed by a failed application.
* A companion designer workspace read returns committed authoring data. Open editor changes, compiler
  diagnostics and local Try interactions have additional state. Publication upload and assignment
  already have native MCP tools. A workspace commit and later host selection have separate outcomes.
* Game launch, host-controller identity reassignment, parts of inventory cleanup, gateway controller
  layout publication, standalone device actions, simulated gateway controls and OS autostart changes
  need typed adapters. Driver inspection already exists. Installing a bundled driver or opening its
  fixed configuration utility also needs an adapter and retains native elevation.
* The MCP administration panel manages external access today. Provider setup, authentication,
  conversation state and trusted approval for the embedded assistant are planned.

## Reading a workflow

Use the running credential's `tools/list` for exact currently callable schemas and the inventory to
trace their effect. A UI file picker maps to validated typed document or bounded upload data. A copy
button maps to the corresponding typed result. Screen navigation, zoom and keyboard focus are local
presentation unless they change retained settings, authoring state, selection or output. Observe a
native revision, owner, device acknowledgement or job outcome before claiming a change.

Manual UI, MCP and provider adapters must share native authorization and availability. For example, new
ESP32 pairing is gated by deferred renderer work and mobile authentication by protected-store and
lifecycle qualification. Both interfaces must expose the applicable reason. Existing software tests
cannot grant physical output readiness.
