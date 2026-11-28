# Core workflow contracts for the embedded assistant

These proposed adapters extend the current tools. All effects follow the [shared execution and approval contract](execution.md).

## Common contract constraints

All new schemas reject unknown fields and enforce existing stricter native bounds. Every non-streaming mutation includes canonical lowercase UUID `operationId`; preserve authenticated-client deduplication and existing `operation_status` semantics. Reads require read access; mutations require existing Configure/write access and shared native admission. Do not introduce generic native command passthrough.

Types used below:

```ts
type Id = string;       // printable, nonempty, <=80 UTF-8 bytes unless native ID is longer
type Uuid = string;     // canonical lowercase UUID, exactly 36 ASCII characters
type Hash = string;     // /^sha256:[0-9a-f]{64}$/
type Revision = number; // integer 1..existing MAX_REVISION, never a float
type Owner = { session: Uuid; generation: Revision; profileId: Uuid;
  profileRevision: Revision; gameId: string | null } | null;
type Page = { offset?: number; limit?: number }; // integers; default 32, maximum 64
type Operation = { operationId: Uuid };
type ErrorCode = 'invalid_arguments' | 'revision_conflict' | 'ownership_conflict' |
  'resource_changed' | 'expired' | 'unsupported' | 'read_only' | 'capacity' |
  'cancelled' | 'native_rejected' | 'operation_conflict';
```

Opaque native keys/routes may retain their existing bounded types (USB key <=256, gatewayId <=100). Owner is always copied from a fresh native status, never invented. Every success reports separate `effects` flags for persisted configuration, runtime output, launch and external process; never collapse saved/applied/uncertain into a boolean. Ambiguous device writes require state reconciliation, not automatic repetition under a fresh operation identity. Async jobs return `jobId`, phase and observation/read endpoint. Stop retains its independent capacity and cannot be delayed by preparing or enumerating jobs.

## Unsaved editor draft lifecycle and Save parity

Use one shared native authoring-session abstraction bound to a visible UI editor when requested, rather than pretending an independent MCP prepared draft has modified the UI. UI and MCP must consume the same revisioned working-copy model if shared-editor parity is claimed. A simpler independent-draft implementation must explicitly report `binding: independent` and be classified partial for visible-editor parity.

Proposed `profile_editor_begin` input:

```ts
type EditorBegin = Operation & {
  binding: 'independent' | 'visible-editor';
  source: { kind: 'blank'; name: string /*1..80*/ } |
    { kind: 'copy'; profileId: Uuid; expectedRevision: Revision } |
    { kind: 'saved'; profileId: Uuid; expectedRevision: Revision };
  expectedEditor: { sessionId: Uuid; revision: Revision } | null;
  replace?: { sessionId: Uuid; revision: Revision };
};
```

`blank` and `copy` return new document identity, `baseRevision:null`, empty local assignments, `saved:false`, `applied:false`. `saved` loads an exact existing revision/assignments. No library slot is consumed until durable save. Reject an existing dirty visible editor unless exact replacement confirmation is supplied. Enforce library capacity at save, and native document limits throughout. Sessions are authenticated-client-owned unless an explicit visible-editor ownership transfer is established; maximum 16 sessions/server, 30-minute idle expiry, aggregate bytes no greater than existing prepared-draft budget. UI work in progress must never disappear merely because an unrelated MCP draft expires.

Proposed `profile_editor_read({sessionId, offset?, limit?})` returns session/editor revision, base saved revision, document/assignment hashes, dirty state, binding, owner/conflict metadata and bounded document chunks. Use digest+byte count paging for large bodies. `profile_editor_change(Operation & {sessionId, expectedEditorRevision, edits})` accepts the existing strict `Edit` union (max 256 edits) and explicit bounded assignment replacement; outputs next editor revision/deterministic diff without durable save or apply. The same full typed ProfileDocument replacement is permitted within the existing 512-KiB portable bound. Preserve undo/redo history when shared editor semantics are promised; expose exact expected-editor-revision `profile_editor_history` action `undo|redo`, rejecting unavailable history rather than inventing inverse edits.

`profile_editor_discard(Operation & {sessionId, expectedEditorRevision})` destroys exactly that draft, cancels its pending preparation and closes/unbinds the matching visible editor; effects remain `saved:false, applied:false`. Reject stale editor identity/revision. Retried exact discard returns original receipt. `profile_prepared_discard(Operation & {draftId})` separately invalidates a current client's existing prepared draft; never deletes its saved source or another editor. It can be idempotent when the same owned draft was already discarded; unknown foreign identity does not disclose draft existence.

`profile_editor_save(Operation & {sessionId, expectedEditorRevision, expectedSavedRevision:Revision|null, expectedActiveOwner:Owner, apply:'save-only'|'replace-own-active'})` persists exact current document and local assignments atomically. `save-only` uses current `profile_draft_save` semantics. `replace-own-active` uses shared `save_profile_editor` behavior while checking the observed owner before beginning. It must replace only its own exact existing active profile, never activate an inactive draft or stop a newer owner. Return committed saved revision and independent `saveApplication:{state:'saved'|'inactive'|'pending'|'applied'|'failed', activationId?, error?}`. Failed application after committed save is not a failed/rolled-back save. Newer unsaved edits arriving during save remain dirty against the returned committed base; they cannot be silently marked saved. Existing `profile_save_prepared` stays save-only unless its schema/description is explicitly changed.

Acceptance: create/copy then discard leaves library and active owner unchanged; visible editor observes MCP edits; competing editor revision rejects; save while active yields UI-equivalent hot replacement and distinct persistence/application outcomes; Stop during slow replacement fences the replacement without undoing committed save.

## Other missing/partial core workflows

| Proposed tool and exact bounded input beyond Operation | Output / effects | Native authority and lifecycle / acceptance |
| --- | --- | --- |
| `game_profile_candidates({gameId})` (read; gameId existing <=96 syntax, no operationId); `game_launch({gameId, expectedCandidatesHash:Hash, profileId?:Uuid, rememberChoice?:boolean, expectedActiveOwner:Owner})` | Candidate page/hash; launch job/acknowledgement plus selected profile/context and activation outcome. Profile starts before executable launch, as UI. | Route through `profile_game_candidates` / `profile_game_launch`, `GameProfileLaunch.svelte`; reject changed candidates/context, unregistered/external executable or owner conflict. No arbitrary launch path. Report profile applied but launch failed separately; no automatic rollback/relaunch. Fixture with two candidates must require explicit choice; compare native running/degraded owner and launched fixture process. |
| `controller_reassign({sourceId:Uuid, observedId:Uuid, expectedInventoryHash:Hash, expectedActiveOwner:Owner})` | Stable reassigned identity and affected references; persisted:true. | Reuse native `controller_reassign` and current source eligibility; require old offline identity/new observed physical evidence, no active conflicting owner. Fresh inventory hash prevents unintended reassignment after USB changes. Confirm exact retry, stale evidence rejection and UI agreement. |
| `devices_cleanup_preview({targets})` read, max 64 discriminated targets `{kind:'controller',sourceId:Uuid}` / `{kind:'gateway-module',gatewayId,moduleId}` / `{kind:'remembered',id}` / `{kind:'companion',companionId}`; `devices_cleanup_apply({previewId:Uuid, previewHash:Hash, selectedIds:string[]<=64, expectedActiveOwner:Owner})` | Preview eligibility/effects/profile references and per-target commit/failure/cancel state. This is a batch of explicit operations, not an all-or-nothing claim. | Shared eligibility implementation with `SystemDevices` / `system-inventory-providers.ts`; native controller_forget/gateway forget/companion revoke, metadata-only targets handled explicitly. Recheck offline/unowned/reference state before each commit. Preview expires within 5 minutes and belongs to client. Cancel prevents remaining work only; no resurrection of committed removals. Test mixed eligible/blocked list and race to connected state. |
| `gateway_controller_change({gatewayId,moduleId,action:'enable'|'disable'|'update-profile'|'register'|'unregister'|'forget', expectedStateHash:Hash, expectedActiveOwner:Owner})`; `gateway_layout_apply({gatewayId,expectedLayoutHash:Hash,expectedActiveOwner:Owner})` | Roster/layout revision, persisted/published flags, reconnect job and retained error; separate pending desired layout from actual USB publication. | Reuse App `changeController`/native enable/disable/update-profile/register/unregister/forget and exact legacy/native apply/reconnect path. Advertise actions only when gateway supports them; cannot forget online/referenced/owned module or migrate stale layout implicitly. Actual descriptor publication and Windows reconnect remain native. Test changed counts remain neutral before update, disabled controller disappears only after successful commit/reconnect, failed publication preserves previous layout. |
| `device_actions_list({target,offset?,limit?})`; `device_actions_state({target,expectedCatalogueRevision})`; `device_action_invoke({target,expectedCatalogueRevision,actionId,values,expectedActiveOwner:Owner})` where target is exact `{kind:'gateway',gatewayId,moduleId,boot}` or `{kind:'usb',key,route}`; actionId <=80 and max 64 typed values governed by advertised native action schema | Typed advertised contracts and actual device state; invocation result explicitly applied/rejected/uncertain. | Reuse ManualActions/action_describe/action_state/action_invoke and USB `module_usb_actions_*`; never arbitrary action string execution. Exact current boot/route/catalogue, output/action lease and profile admission required. Refresh after uncertainty; readback is not physical output verification. Test unsupported firmware, stale catalogue, output readback and no duplicate physical action on exact retry. |
| `debug_gateway_describe({gatewayId,offset?,limit?})` read; `debug_device_create({gatewayId,preset,expectedRosterHash:Hash})`; `debug_device_change({gatewayId,moduleId,boot,action:'online'|'offline'|'reboot'|'direct-usb-on'|'direct-usb-off'|'neutral',expectedRosterHash:Hash})` | Advertised preset/roster, boot/route lifecycle receipt. No hardware claim. | Only gateway that explicitly advertises debug backend; max presets/devices remains native. Preset <=80 from discovered enum. Do not accept generic debugOperation or untyped args. Reboot invalidates boot; teardown best-effort neutral must not target newer boot. Test add/offline/reboot/direct-route changes through UI state. |
| `debug_device_input({gatewayId,moduleId,boot,streamId:Uuid,sequence:string,changes})` streaming, no operationId; changes max128 discriminated raw button/axis/hat/encoder/selector values validated against advertised controls (axis integer 0..4095) | Accepted sequence and raw/calibrated state; effect: simulated input only. | Share bounded ordered input queue and strictly increasing canonical decimal sequence semantics with authored debug streams, while retaining separate simulator identities. Exact retries harmless, old sequence rejected, backpressure explicit. Add explicit neutral action and release reader/stream on close; no output-owner heartbeat from debug input. Test calibration pipeline and stale boot/sequence rejection. |
| Optional `profile_select({id:Uuid,expectedRevision:Revision,expectedActiveOwner:Owner})`; optional `profile_reset({expectedLibraryHash:Hash,expectedActiveOwner:Owner})` | Selection-only or reset receipt and fresh library/runtime state. | No confirmed visible UI caller; these are optional native inventory completion, not mandatory portal parity. Reuse native profile admission, refuse unreviewed library/owner changes. Reset must disclose native runtime consequences in preview; do not invent safer semantics absent native support. |

All action sets above are closed enums. Inventory/preview hashes must be issued by native inspect responses from canonical exact state, not client-computed approximations. New previews should contain identities, effects and errors useful for review; no opaque generic command payload.

## Installers and system provisioning coverage


Confirmed current UI/native surfaces missing from MCP:

- `src/lib/OptionalDependencies.svelte` invokes `optional_dependencies` then `optional_dependency_install({id:'vjoy'|'hidhide'})`. Native commands are in `src-tauri/src/lib.rs:280-306`; implementation `src-tauri/src/optional_dependencies.rs::list/install` reads the bundled manifest, checks installer size/SHA-256, restricts Windows x64 and serializes installation. Windows elevation/cancellation/restart are native outcomes. Success merely prompts fresh inspection and does not enable mapping/hiding.
- `OutputSettings.svelte::manageDriver` invokes `open_vjoy` / `open_hidhide`; `optional_dependencies.rs::open_vjoy/open_hidhide` launches only fixed installed upstream configuration clients with Windows elevation, never a caller-supplied path. That external application owns its own edits; opening it is not proof of completed driver/layout configuration.
- No separate Cockpit portal uninstall or repair command was found. The interactive pinned installer may offer maintenance UI; do not claim audited native uninstall/repair APIs. No general software installer manager is exposed in the inspected portal. Add future contracts only when native workflows actually exist.
- vJoy provisioning code exists in `crates/virtual-controller/src/{provision.rs,windows_provision.rs,windows_provision_entry.rs}`; its presence does not establish a separate manual portal provisioning entry point. Profile activation may exercise native provisioning under existing gates. Retained provisioning recovery/cancel is already exposed through `profile_retired_recovery_*`. Settings currently launches the external vJoy app for layout management. Do not label generic machine-level vJoy create/delete as supported MCP operations.

Proposed bounded contracts:

| Tool | Input / output / admission | Acceptance |
| --- | --- | --- |
| `optional_dependencies_list` (read) | Empty input; return <=2 bundled IDs, pinned version/hash/size, platform support and inspected installed state/revision. Do not return bearer secrets or accept URLs/paths. | Compare native manifest and Settings cards; unsupported platform remains explicit. |
| `optional_dependency_install` | Operation plus `{id:'vjoy'|'hidhide',expectedManifestHash:Hash,expectedSystemRevision:Hash,expectedActiveOwner:Owner}`; single async job with phase `preflight|awaiting-consent|installing|completed|cancelled|failed|restart-required`, fixed package identity, exitCode and message. Shared native installer verifies bundled bytes, closes/blocks conflicting active output/setup per native admission, and serializes jobs. UAC remains interactive; an MCP call cannot silently supply consent. | Fixture verify hash mismatch, competing job, UAC cancel, success/restart-required, then fresh system inspection. Installer return is not device qualification. Actual administrator installation should be separately explicitly authorized in acceptance. |
| `optional_dependency_job` (read) | `{jobId:Uuid}`; exact retained job/status, no arbitrary PID. On server restart inspect system state, never infer rollback/re-run an unknown installer. | Reconnect during job and reconcile status; no duplicate launch on exact retry. |
| `optional_dependency_open_config` | Operation plus `{id:'vjoy'|'hidhide',expectedSystemRevision:Hash}`; fixed installed client open receipt/OS cancellation. Return `configurationVerified:false`; no completion claim until fresh native inventory. | Fixture validates fixed-path selection/no caller path, missing app and cancelled elevation; UI/MCP agree on launch result. |

Do not add fake cancellation for upstream installers unless the native implementation can safely offer it. A future native provisioning API would require exact slot/layout preview, driver/system revision, external owner exclusion, explicit confirmed commit, distinct created/updated/restart/uncertain outcomes and retained cleanup receipts; external-app launch alone does not satisfy that contract.
