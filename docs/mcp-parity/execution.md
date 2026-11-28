# Shared execution, provider and event contracts

These interfaces are implementation requirements for [execution and approval](https://github.com/opensimgear/cockpit/issues/172),
[typed adapters](https://github.com/opensimgear/cockpit/issues/173) and
[providers](https://github.com/opensimgear/cockpit/issues/174). Existing external MCP callers continue to
discover the running server's catalogue and use its operation ledger. The interfaces below describe the
additional native policy boundary required by those milestones.

## Admission and execution

UI, external MCP and provider tool calls resolve the same registered workflow and typed arguments.
The native dispatcher supplies identity, access scope and generations from its authenticated context.
It resolves the authoritative command from the catalogue. The caller cannot supply a manager, command
string, approval decision or native authority. Direct adapters such as listener administration and
credential replacement enter the same policy boundary as gateway-backed mutations.

| Interface | Input and result |
| --- | --- |
| Discover | Domain and bounded page → tool names, exact JSON schemas, effects, native availability and reason. Filter by credential and platform. The UI can render the same reason returned by MCP. |
| Inspect / prepare | Validated typed arguments and current snapshot → bounded data, or an immutable proposal containing canonical tool and arguments, effect, target, expected revisions, route/owner, issuer, run/provider generation and expiry. Read-only compilation can produce a draft handle scoped to the requester. |
| Submit | Tool, schema-checked arguments and operation UUID → completed readback, native rejection, pending job, or awaiting-approval proposal ID. A retry with the same identity and payload joins the same operation. An altered payload conflicts. |
| Decide | Trusted desktop UI receives a native-rendered summary of target, effect and current state. A human allow/deny decision is bound to the exact proposal. Consume an allow decision once, rechecking every guard under the owner's commit admission. Denial, expiry or stale state ends the proposal. |
| Observe / reconcile | Operation ID → state, bounded progress, cancelability and native result/reference. After a disconnect query the original operation and authoritative target. An unknown outcome remains uncertain until readback resolves it. |
| Cancel / stop | Exact job/run/owner identity → cancellation requested or terminal state. Keep capacity for urgent stop independent of approval and tool queues. Once a native commit passes its cancellation boundary, finish reconciliation and report its actual outcome. |

Classify each concrete request by its most consequential effect. Inspecting device input is a read.
Saving a portable draft is routine configuration. Applying it can drive live output. Saving an automatic
rule can grant future activation. Installer/file writes and OS registration use native preflight and
elevation. Deletion, reset, restore, destructive replacement and changes to access require trusted human
confirmation in automated execution. An operation that changes credentials also rechecks its listener
generation and scope at commit. Pairing decisions, consent and elevation use their native human surfaces.
An external caller can initiate a request, observe its state and cancel while pending.

Disablement, revocation, provider switch, sign-out, shutdown and restore invalidate queued proposals for
the retired generation. Restart does not restore approval authority. Already executed changes retain
their real native outcome. Readback uses saved revision, runtime owner, device acknowledgement or
installer result as appropriate. Audit only operation IDs, workflow, safe target metadata, decisions
and terminal outcome. Omit prompt bodies, credential material, pairing payloads and uploaded bytes.

## Provider boundary

| Interface | Inputs | Output and owner |
| --- | --- | --- |
| Configure / probe | Provider kind, validated endpoint or installed ACP identity, protected secret-store reference, timeout | Native provider settings return safe status and observed model/stream/tool capabilities. A failed probe preserves the previous selection. |
| Authenticate | Provider ID, expected settings generation, start/status/cancel/logout | Native auth manager owns browser consent, PKCE state, refresh and protected storage. Return expiring session ID, safe account status and reason. Late callbacks for retired sessions cannot select an account. |
| Start turn | Conversation ID, run UUID, provider generation, bounded history, user message and catalogue page | Provider adapter streams text and structured tool requests into a bounded native run. The adapter assembles and validates arguments before handing them to shared execution. |
| Resume tool result | Run UUID, provider generation, call ID, bounded structured result | Adapter delivers only the matched result. Provider text and tool output carry no authorization. ACP uses a managed restricted session. |
| Stop / retire | Run UUID and generation | Cancel inference and pending uncommitted calls. Report committed, pending or uncertain effects separately. Reject delayed provider fragments. |

Subscription OAuth, API-key endpoints, local Ollama and ACP implement these same operations.
Each advertises its actual capabilities and errors. A provider change requires a new generation.
Credentials remain in protected native storage. The client receives identifiers and safe status only.

## Events and acceptance

Each event carries a run or operation ID, generation, monotonically increasing sequence, kind and
bounded typed payload. Kinds cover text delta, tool proposal, approval requested/decided, progress,
tool result, provider error and terminal outcome. Native state records the latest snapshot. On sequence
loss or reconnection the renderer reloads the snapshot before displaying later events. A closed chat
panel can reattach to the same run. New conversation retires the previous chat context explicitly.
Use the existing observation service for native state and the existing mutation ledger for effects.

The acceptance fixture submits identical guarded mutations from an external SDK client and an embedded
provider stub. Compare authoritative revisions, recovery behavior and readback. Exercise stale owner,
duplicate UUID with changed arguments, denial, expiry, a late provider callback after sign-out, and a
disconnect on either side of commit. Each rejected proposal leaves the target revision unchanged.
A committed write remains observable after chat cancellation. A restricted ACP tool request must pass
the same native admission. The renderer reports saved, running, pending, failed, cancelled and uncertain
outcomes separately.
