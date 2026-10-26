# MCP contract and acceptance matrix

This is the implementation contract for [feature #119](https://github.com/opensimgear/cockpit/issues/119).
See [client setup and workflow](mcp.md) for the user-facing sequence. The server
adapts the running desktop's existing `Arc<GatewayManager>`; it does not create
another device owner or service process.

## Exact schemas and response envelope

`tools/list` returns each tool's exact `inputSchema`, shared `outputSchema`, and
read/write annotations. Top-level arguments reject unknown fields. Nested
portable documents, edits, assignments, runtime configs, and processing-operation
parameters use schemas derived from the native Rust types in
[schema.rs](../src-tauri/src/mcp_tools/schema.rs). The native parser, compiler,
encoder, and coordinator remain authoritative after schema validation.

The canonical definitions are:

| Contract | Source / runtime resource |
| --- | --- |
| Tool names, required/optional arguments, limits, access classification | [catalogue.rs](../src-tauri/src/mcp_tools/catalogue.rs), `tools/list` |
| Portable profile, typed edits, local assignments, module config | [schema.rs](../src-tauri/src/mcp_tools/schema.rs), `cockpit://schemas/profile`, `cockpit://schemas/local-assignment` |
| Processing operation parameter schemas | `processing_operations_list.operations[].parametersSchema`; per-operation `schemaHash` |
| Success/error envelope | `cockpit://schemas/task-results`, each tool's `outputSchema` |
| Workflow, current limits, local-setting field sets | `cockpit://guide/configuration` |
| Transport versions, settings and credentials | [server](../src-tauri/src/mcp/server/mod.rs), [credentials](../src-tauri/src/mcp/storage/credentials.rs) |

Successful tool replies contain `{ok:true,data:…}`. Mutating results also identify
their `operationId`. Failed tool replies contain
`{ok:false,error:{code,message,retryable,guidance}}` and set MCP `isError`.
An optional operation ID is a string when present; it is not `null`. HTTP-level
authentication and protocol errors may occur before any tool result exists.
Clients must retain and inspect structured errors instead of treating them as
successful saved/applied state.

Every mutation requires a fresh canonical lowercase UUID `operationId`.
Replaying the exact tool name and arguments with the same authenticated
credential returns its retained pending/completed result; a changed payload
returns `operation_conflict`. Different credentials have independent operation
and draft identities. Reconnecting with the same credential preserves those
identities for the running server object's lifetime. Restart requires native
state reconciliation before fresh operation IDs are issued.

## Tools to native commands

`page` below means optional `offset`/`limit` arguments, generally bounded to
64 entries; individual inventories have their own smaller limits. `owner` means
the exact `profileSession.owner` observed from `profile_status`, or explicit
`null` when stopped. Never manufacture it from profile IDs or a selected item.
All write rows also require `operationId` unless noted as passive preparation.

| MCP tools / access | Required selectors and important shape | Shared native dispatch / invariant |
| --- | --- | --- |
| `profiles_list`, `profile_get`, `profile_status`, `profile_export` · read | `page`; `id` for get/export | `profile_list`, `profile_assignments_get`, `profile_status`, `profile_export`. Get includes saved document and local assignments; export excludes machine-local assignments. Selected, saved, and applied state stay separate. |
| `profile_create`, `profile_duplicate`, `profile_import`, `profile_rename`, `profile_delete` · write | Name, source `id` or typed document as advertised; rename/delete require `expectedRevision` | Same named `profile_*` commands. Native bounds, fresh-copy/import identities, Default protection, revision checks, storage validation, and deletion admission apply. No implicit reset. |
| `resources_list`, `resource_inspect`, `resource_search_ports` · read | `page`; exact `resourceId` for inspect/search; search text | `profile_resources_list`, `profile_resources_inspect`. Search scans one exact bounded resource and rejects changing capability stamps; retained metadata never grants writer authority. |
| `processing_operations_list` · read | Optional `search`, category enum, `page` | Native `Operation` type definitions, expanded parameter schemas, stable hashes. No execution or output acquisition. Compatibility still requires compiler validation of declared ports. |
| `games_list`, `game_discovery_status`, `game_vehicles_list` · read | `page`; discovery status requires exact `scanId`; vehicles require `gameId` plus optional `search` | `game_get_state`. Game summaries omit nested vehicle arrays and provide a lookup tool/count; discovery status pages the current scan snapshot and rejects stale scan IDs; vehicle pages carry retained catalogue/setup stamp and current context without selecting a vehicle or starting a provider. |
| `game_discovery_start`, `game_discovery_apply`, `game_save`, `game_remove`, `game_installation_select` · write | Start accepts optional `paths`; status uses `scanId` and page; Apply requires exact `scanId` and bounded typed `actions`; metadata changes require exact game/installation selectors. Every write uses `operationId`. | Shared native game discovery starts asynchronously and returns the scan identity/snapshot immediately. Status exposes progress, candidates, defaults and diagnostics. Apply atomically revalidates and commits selected Add/Update/Remove actions. Save edits one discovered game's metadata, Remove deletes one registration, and installation selection updates only the preferred installation; none launches a game. Writes return bounded receipts, without catalogue images, so operation replay remains stable. Discovery pages expose `hasArtwork` and omit image bytes. |
| `game_telemetry_inspect` · read | Optional game/search/group and `page` | `game_telemetry_inspect`. Passive bounded current/cached channel inspection; no provider start. |
| `game_aircraft_resource` · write | Exact installed `vehicleId` | `profile_dcs_vehicle_resource`. Explicit aircraft catalogue registration changes inventory discovery; accepts no file path and does not activate a provider/profile/game. |
| `companions_list`, `companion_panels_list`, `companion_inspect` · read | `page`; inspect requires native resource ID and immutable publication hash | Native resource inventory plus `profile_companion_catalogue` / `profile_companion_inspect`. Device identities and retained verified panel publications remain distinct. |
| `widgets_list`, `widget_get` · read; `widget_save` · write | List supports `offset`/`limit`; get requires `id`; save requires plain `{definitions:[source]}` with exactly one definition, the library `expectedRevision` returned by list/get, and `operationId` | Shared native widget library. Full native source and asset validation, replacement by stable ID and atomic persistence. Authoring has no format/revision fields or compatibility reader. Read `cockpit://guide/widgets` for the source example. Saving never publishes or activates a panel. |
| `profile_prepare_changes` · read | Explicit null/numeric `expectedRevision`; existing `profileId` plus typed edits, full typed document, or new name; optional assignments | Reads the saved base/assignments, applies typed edits, reparses, validates assignments, calls `profile_compile`, and retains a client-owned draft. Returns diff, document/assignments, validation, compilation, and `effects` with no save/select/apply. |
| `compilation_inspect` · read | Exact `compilationId`, section enum, `page` | `profile_compilation_inspect`. Bounded sections from a recent cached native preview; it is not Apply authority. |
| `profile_save_prepared` · write | Exact client-owned `draftId` | `profile_draft_save`. One atomic native document+assignment transaction checks the captured expected revision. It leaves the applied owner/revision unchanged. Existing UI `profile_editor_save` retains its reapplication behavior. |
| `profile_apply` · write | Saved `id`, `expectedRevision`, `expectedActiveOwner`; optional logical `gameId` | `profile_apply`, with `activationId` taken from `operationId`. Fresh staged preparation, resource/owner/revision revalidation, native output gates, takeover, and compensation apply. |
| `activation_status`, `activation_current` · read; `activation_cancel` · write | Status/cancel use exact `activationId` | Corresponding `profile_activation_*` commands. Progress/cancel remains independent of slow admission; cancellation targets that activation, not a newer owner. |
| `profile_stop`, `profile_recover` · write | Explicit operation identity | Same named commands, native cleanup receipts and owner lifecycle. Stop/Recover do not replay a graph; cleanup cannot be cancelled halfway through. |
| `applied_ports_inspect`, `applied_topology_inspect` · read | Exact `owner`; optional `nodeId` or required nodes/edges section; `page` | `profile_runtime_inspect`, `profile_runtime_topology`. Saved or selected profiles cannot impersonate the captured applied runtime. |
| `system_inventory` · read | Optional bounded page and captured `observedAt` | `system_inventory`. Read-only native device/controller/virtual/hidden observations. |
| `gateways_list`, `gateway_inspect`, `gateway_config_read` · read | Exact discovered `gatewayId`; config also requires `moduleId` | `ports`, `status`, `config_read`. The same adapter/state used by Devices remains authoritative. |
| `gateway_session`, `gateway_config_change` · write | Exact gateway/module; advertised action enum; current owner; full config for apply actions | `connect`/`disconnect`; `claim`/`release`, `config_apply`, `config_apply_save`, `config_save`, `config_discard`, `config_reset`. Native boot, route, writer, lease, profile admission, USB preference, and cross-transport exclusion remain enforced. |
| `usb_modules_list`, `usb_module_inspect`, `usb_module_config_read` · read | Native key; config requires the exact current route token | `module_usb_list`, `module_usb_status`, `module_usb_config_read`. Keys, module ID, serial, boot, connection generation, and configuration generation are observed native evidence. |
| `usb_module_session`, `usb_module_config_change` · write | Exact key/route; observed port for connect; action enum; current owner; config for apply | `module_usb_connect`/`disconnect`, `module_usb_claim`/`release`, `module_usb_config_apply`, `save`/`discard`/`reset`. Apply-and-save sets explicit native save intent; stale routes fail closed. |
| `device_config_prepare` · read | Full typed `config`; transport selector plus exact gateway/module or USB key/route | Native `config_read` or `module_usb_config_read`, native config encoder validation, deterministic diff. No claim/apply/save; firmware semantic acceptance and fresh writer checks remain Apply outcomes. |
| `local_settings_get`, `local_settings_save` · read/write | Scope enum; game ID for game scopes; saves require snapshot `expectedSettingsRevision`, exact owner and scope-specific settings | `profile_local_settings_get`/`save`. Shared admission validates a current native settings snapshot and owner before provider/desktop/automatic policy persistence. Paths remain local preferences. Automatic mode/rule edits may explicitly authorize runtime activation. |
| `operation_status` · read | Original `operationId` | Adapter's per-credential bounded operation ledger. Unknown/restarted ledger means inspect actual native state; do not replay with a fresh ID blindly. |

MCP profile reads do not persist follow-latest companion panel refreshes. Draft
Save rejects `resource_changed` if native panel normalization would substitute
references for the exact reviewed document; refresh metadata and prepare again.

The exact local-setting scopes are `game-provider`, `desktop`, `selection-mode`,
`game-rule`, and `game-preference`. Their native field sets are documented by the
workflow resource and enforced in
[profile_local_settings.rs](../src-tauri/src/profile_local_settings.rs).
Game IDs in saved rules and profile eligibility use lowercase ASCII letters,
digits and hyphens, are 1–96 bytes, and start with a letter or digit. Recognized
IDs such as `dcs` and `x-plane-11` remain canonical game identities; discovery
may also register any syntactically valid unknown ID without a custom prefix.

## Error and lifetime invariants

| Condition | Structured result / required next action |
| --- | --- |
| Unknown tool/argument, malformed document/edit/assignments/config | `unknown_tool`, `invalid_arguments`, `invalid_document`, `invalid_edit`, or `invalid_assignments`; repair the proposal, without a native write. |
| Changed saved revision, local snapshot, resource capability, or owner | `revision_conflict`, `resource_changed`, or `ownership_conflict` as classified by native rejection; inspect fresh evidence and review again. |
| Expired/foreign draft or preview, missing native identity | `draft_expired`, `expired`, `not_found`, or native rejection; re-inspect/reprepare rather than reconstructing authority. |
| Disabled/revoked/read-only credential or stale listener generation | HTTP 401 before dispatch, protocol access rejection, or structured `access_denied`; reconnect only with authorized current access. Pending native admission retains its final authorization fence. |
| Full concurrency, draft, or mutation-ledger budget | `busy`, `capacity`, or request/reply size errors; bound/paginate work. Busy preserves the original operation identity for reconciliation. |
| Native panic, timeout, disconnected reply, or uncertain commit | `native_failure`, `outcome_pending`, or transport failure; inspect original `operation_status` / activation status and current native state. Cancellation is not proof of rollback. |
| Store corruption or failed native persistence | `read_only`, `native_rejected`, or native storage diagnostics; preserve the previous committed state and report the actual outcome. |

Native text failures are mapped to these stable categories in
`mcp_tools::native_result`; detailed native validation messages remain available.
No error grants permission to bypass a profile/output/hardware qualification
gate. Drafts expire after 30 minutes; 16 retained drafts share an 8 MiB budget.
The operation ledger admits 16 mutation clients and 128 operations per client
without deduplication eviction. Request payloads are bounded to 2 MiB; inspection
and tool replies have separate bounded envelopes. See the runtime workflow
resource for the current constants.

## Acceptance coverage

This table identifies the intended evidence paths; consult the verification
record for actual commands, results, application/client versions, and limits.
The presence of a test file does not itself establish that a run passed.

| Milestone / invariant | Implementation and verification paths |
| --- | --- |
| M0 schemas, typed arguments, catalogue completeness, bounded errors | `src-tauri/src/mcp_tools/catalogue.rs`, `schema.rs`, `tests.rs`; `tests/ui/mcp-software-scenarios.mjs` exercises actual operation schemas and paged game/vehicle discovery. |
| M1 one running service, authentication/scopes, host/origin rejection, lifecycle | `src-tauri/src/mcp/server/tests.rs`, `mcp/storage/credential_tests.rs` native tests; `tests/ui/mcp-native.mjs` uses the official SDK and actual native WebView IPC. |
| M1 connection UI and one-time credentials | `src/lib/mcp.test.mjs`, `mcp-ui.test.mjs`; native UI acceptance verifies visible listener/credential state without displaying stored tokens. |
| M2–M3 discovery, explicit transformation/wiring, read-only preparation and diff | `mcp_tools/tests.rs`, `mcp_tools/authoring.rs` tests; live software scenarios save a constant → NOT → NOT graph and compare native compiler traces and rendered nodes/routes. |
| M4 atomic draft-only persistence and concurrent writers | `crates/profiles/src/service_editor_tests.rs`, `src-tauri/src/profile_editor_save_tests.rs`, `mcp_tools/tests.rs`; live helper races independently authenticated writers at one revision and verifies exactly one commit. |
| M5 explicit activation/Stop/cancel/owner and revision fencing | Native coordinator/activation tests and `tests/ui/mcp-native.mjs`; compare applied native owner with the real UI, and keep durable save separate from applied revision. |
| M6 local settings and device boundaries | `profile_local_settings.rs`, provider/automatic settings tests, adapter tests; live helper edits/readbacks/restores a passive provider preference and verifies a stale second writer conflicts. Device/simulator evidence is recorded separately. |
| M7 client workflow, restart, packaging, and declared release scope | `tests/ui/mcp-native.mjs`, `mcp-software-scenarios.mjs`, and dated `docs/verification/` records. Record software, native simulator, packaged startup, and physical hardware evidence separately. |

Physical hardware validation for this feature is deferred by the user. This
matrix does not graduate electrical CAN, physical USB/HID, simulator bindings,
or output qualification.
