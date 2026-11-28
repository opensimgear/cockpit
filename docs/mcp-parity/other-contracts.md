# Companion, settings and planned AI contract details

These are proposed contracts, not tools already implemented. Current evidence comes from committed cockpit HEAD. They complement the shared execution contract; use its common result envelope, authenticated principal, operation identity, admission, observation and cancellation rules. All request objects reject unknown fields recursively. Bounds below are proposed acceptance limits, not claims about current implementation.

## Current authoring semantics and corrections

- `src/lib/CompiledCompanionDesigner.svelte:157-165` opens a saved panel by cloning it into local `draft`; New makes a local draft and sets `dirty`. `change`, undo and redo alter that clone. These edits are absent from `designer_workspace_read` until explicitly saved. Local widget edits likewise live inside `GenericWidgetDesigner.svelte` source history. Existing workspace tools read/write committed authoring data, not an open editor's unsaved buffer.
- The ordinary panel UI Save requires a current compiled result (`saveDisabledReason`, line 41). Save captures the panel, commits workspace/source/publication, then calls `synchronizePanel` in `src/lib/panel-actions.ts` to publish/select. These are sequential operations: a later publication/selection failure can leave authoring data committed. Do not call this a single transaction spanning workspace and host assignment.
- `importPanel` persists the imported source draft without its publication, then opens it dirty with “Compile this panel.” Native workspace validation permits 0 widgets, and a panel without a saved publication. Thus MCP can create/edit/delete a **saved panel draft** via full workspace commit without compiling or assigning. That is stronger than the ordinary UI Save affordance, but does not provide unsaved-editor introspection, compile or Try.
- `widget_save` is an upsert: `src-tauri/src/companion/storage/widgets.rs:37-53` replaces an existing source ID or appends a new source. A dedicated `widget_create` task is unnecessary for persisted creation. Native workspace write also merges submitted sources with the existing library (`workspace.rs:170-172`), then writes the union into the workspace. **Omitting a source does not delete it.** There is no library-source Delete button in the current `CompiledCompanionDesigner.svelte` Widgets list and no native/MCP delete operation. Removing a placement or scene layer is a different supported operation. Widget deletion is a future product capability, not an existing manual/MCP parity defect.
- Native workspace admission allows 256 sources / 8 MiB library, 20 panel drafts, 1–16 pages/draft, 0–192 placements/draft, and at most 100 archived publication revisions (`workspace_validation.rs`). Current frontend workspace validation is stricter: one compiled presentation per panel and 20 overall (`compiled-companion-library.ts`). A native-accepted workspace can therefore exceed frontend reopen limits. Proposed shared authoring contracts must use one authoritative bound or reject such snapshots before commit; do not label upload success alone as successful UI parity.

The existing chunked upload has workspace and library revision fences, a content digest, exact offsets,
bounded bytes and an atomic database commit. Reuse it for persisted authoring. The editor-session
operations below cover the unsaved panel or widget in the mounted editor.

| Proposed operation | Typed request | Readback and admission |
| --- | --- | --- |
| Open or inspect | Open takes an operation UUID, kind (panel or widget), source (blank or saved ID), expected workspace/library revisions and optional exact visible editor ID. Inspect takes the returned session UUID. | Return kind, session and editor revisions, saved base, dirty flag and bounded draft. Check caller ownership and existing dirty editor before attachment. Refuse replacement unless its exact draft is explicitly discarded. |
| Edit, undo or redo | Session UUID, expected editor revision, operation UUID and a discriminated edit: replace with a validated widget source or panel draft, undo one step, or redo one step. Reuse workspace upload limits for large documents and assets. | Advance the editor revision and notify the attached view. Report dirty state plus undo/redo availability. A stale edit leaves both drafts intact. |
| Discard | Session UUID, operation UUID and exact editor revision. | Retire the owned session. Dirty discard uses trusted review. Return the saved base revision. Neither library revision nor panel selection advances. |
| Save | Session UUID, operation UUID, expected editor, workspace and library revisions. Panel Save additionally binds the compiler result hash, target and observed host selection token. | Delegate to existing workspace/widget storage and guarded publication. Return authoring commit, publication hash and selection outcome separately. Reconcile a committed authoring save before retrying host selection. |

Open an unsaved widget in the mounted UI, edit it through the attached session and confirm that
the library read still returns the saved source. Reject a stale concurrent edit. Discard and verify
that the stored revision is unchanged. Repeat with Save and compare the returned revision with the
workspace observation. A second client must be unable to read or discard the first client's private draft.

## Compile and local Try

Common types: `Id` is 1–64 printable ASCII characters matching current designer IDs; `Hash` is `sha256:` plus 64 lowercase hex characters; `Revision` is a safe nonnegative integer; `Scalar` is boolean, finite number or UTF-8 string at most 120 characters. Compile admission reuses current source, draft, capability and asset validators. No filesystem paths, executable code strings, arbitrary worker URLs or device target IDs are accepted.

| Proposed task | Strict input | Success data | Boundaries and negative outcomes |
| --- | --- | --- | --- |
| `designer_compile_start` | `{operationId, input: {kind:"widget", source:WidgetSource, parameters:Record<Id,Scalar>, width:int[1,8192], height:int[1,8192]} \| {kind:"panel", draft:CompiledDraft, sources:WidgetSource[0..256]}, target:{kind:"browser", motion:boolean, audio:boolean}}`; serialized authoring input <=8 MiB; panel retains 192 placements/16 pages | `{compileId:UUID,inputHash:Hash,targetHash:Hash,state:"queued"}` | Pure compile job owned by caller; no workspace save, publication, assignment or output authority. Exact typed union; reject unsupported target, invalid source/binding, stale referenced content, excessive resource estimate, exhausted capacity. ESP32 is not silently substituted. |
| `designer_compile_status` | `{compileId:UUID}` | `{state:"queued"\|"running"\|"completed"\|"failed"\|"cancelled", completed:int,total:int, diagnostics:Diagnostic[0..256], result?:{artifactId:UUID,artifactHash:Hash,byteLength:int,resources:ResourceSummary,fallbacks:Fallback[0..64]}}` | IDs scoped to requesting principal; unknown/expired/wrong owner rejects. Diagnostics carry source/node/property path, severity and bounded message; cap reply at 256 KiB and report truncation explicitly. |
| `designer_compiled_read` | `{artifactId:UUID,expectedHash:Hash,offset:int>=0}` | `{hash,byteLength,offset,base64,nextOffset:int\|null}`; <=48 KiB decoded chunk | Immutable artifact bytes; reject wrong hash, offset beyond length, expired/wrong-owner artifact. Reading never publishes. |
| `designer_compile_cancel` | `{operationId,compileId:UUID}` | `{state:"cancelled"\|"already-terminal",terminalState?:string}` | Cancels exact job only; completed artifact remains immutable. No new result may be published from a worker after its cancellation generation. |
| `designer_try_begin` | `{operationId,artifactId:UUID,expectedHash:Hash,pageId?:Id}` | `{tryId:UUID,state:"ready",sequence:0}` | Fresh simulator state; never host assignment, native input, device dispatch or game feedback subscription. Artifact must be successful/current and compatible with browser simulation. |
| `designer_try_step` | `{operationId,tryId:UUID,expectedSequence:int>=0,events:TryEvent[1..64],advanceMs:int[0,1000]}`; `TryEvent` is declared input event with typed values, declared feedback injection with quality/provenance, or local page selection | `{sequence,requestedOutputs:Output[0..256],regions:RegionState[0..128],trace:Trace[0..256],traceTruncated:boolean}` | Deterministic bounded simulator clock/work; reject unknown event/signal/page, wrong type/range, contradictory feedback quality, stale sequence and >10,000 ms cumulative run. Requested intent is never described as game-confirmed output. |
| `designer_try_end` | `{operationId,tryId:UUID}` | `{state:"ended"\|"already-ended"}` | Reset gesture/output/simulation state. Wrong owner rejects. End on expiry/cancel/server stop. |

Proposed capacity: 2 concurrent compile jobs/client, 8 globally, 32 MiB aggregate retained artifacts, 10 minute job/artifact TTL and 2 minute idle Try TTL. Enforce admission before allocating; resource failure returns `capacity_exceeded` without evicting another active owner's job. Apply latest source/draft identity fences when attaching results to an editor. Source edits reset Try and invalidate UI compilation as today. Test invalidation/cancellation, deterministic steps, reply paging, expiry and zero native device commands.

## OS autostart

Current UI calls native `set_autostart` from `DesktopPreferences.svelte`; `settings.autostart` observes real files/registry (`desktop/autostart_*`). This is a missing MCP mutation, not a preference key. Proposed shared service:

- `desktop_autostart_get({}) -> {supported:boolean,enabled:boolean|null,registrationHash:Hash|null,error:PublicError|null}` reads actual OS registration and known executable identity. Read never registers anything.
- `desktop_autostart_set({operationId,enabled:boolean,expectedRegistrationHash:Hash}) -> {supported:true,enabled:boolean,registrationHash:Hash}` atomically checks currently observed registration identity, changes only Cockpit's fixed registration and re-reads the result. No program path, arguments, registry path or desktop file text is caller-controlled.
- Unsupported OS => `unsupported_platform`; unreadable/ambiguous current registration => `registration_unavailable`; external change => `registration_conflict`; OS write denied => `permission_denied`; success cannot be reported until native observation confirms requested registration. On uncertain OS outcome return `reconciliation_required` and fresh read guidance. Disabling does not kill Cockpit. Never route this through `ui_preferences_save`.

The existing UI native command currently accepts a boolean without this hash fence. The shared implementation must add the fence for both UI and MCP, rather than inventing MCP-only protection. Test external registry/file edits, denied writes, unsupported OS and reopen observation.

## Planned AI settings and provider credentials

No current AI provider/config/auth/chat implementation was found. Existing MCP bearer credentials authenticate incoming clients; companion pairing authenticates remote screens. Neither is provider login. Proposed values are intentionally separate:

```ts
type ProviderKind = "openai-compatible" | "ollama" | "acp" | "subscription-oauth";
type AiConfig = {
  enabled: boolean;
  providerId: string | null; // configured provider entry ID, <=64 chars
  modelId: string | null;    // <=128 chars, from adapter model catalogue
  approvalMode: "task-policy"; // use each native task's approval policy
  sendContext: "selected-only";
  maxOutputTokens: number;  // integer 256..32768, bounded by model
};
type PublicAuth = {
  providerId: string;
  state: "signed-out" | "authorizing" | "ready" | "expired" | "unavailable";
  credentialId: string | null; // opaque ID, never token/secret/hash
  accountLabel: string | null; // <=120 chars, redacted optional display
  reason: string | null;       // <=500 chars, redacted
};
```

| Proposed task | Strict input | Success data | Boundaries and negative outcomes |
| --- | --- | --- | --- |
| `ai_settings_get` | `{}` | `{revision,config:AiConfig,providers:PublicProvider[0..16],auth:PublicAuth[]}` | Public metadata only; no secrets or transcripts. Supports the four provider kinds below. |
| `ai_settings_save` | `{operationId,expectedRevision:Revision,config:AiConfig}` | `{revision,config}` | Validate adapter/model combination and output bound; reject stale revision, unknown provider/model or unsupported approval mode. No network request or silent login; enabling with missing auth yields explicit unavailable readiness. |
| `ai_models_list` | `{providerId:string<=64,cursor?:string<=256,limit:int[1,64]}` | `{models:PublicModel[],nextCursor:string|null,catalogueRevision:Hash}` | Configured supported provider only; model IDs and capability/token limits. Network/auth errors distinguish unavailable catalogue from empty list. Bounded cache TTL and explicit refresh policy. |
| `ai_auth_begin` | `{operationId,providerId:string<=64,method:"oauth"}` | `{authId:UUID,state:"awaiting-user",authorizationUrl:string<=2048,expiresAt:UTCms}` | Adapter-owned fixed HTTPS authorization endpoint, PKCE/state and native fixed callback. No caller redirect/URL/scopes. OAuth browser login is explicit. Refuse unsupported auth method, unavailable secure store, existing active flow or unavailable provider. |
| `ai_auth_status` | `{authId:UUID}` | `{state:"awaiting-user"\|"complete"\|"failed"\|"cancelled"\|"expired",auth:PublicAuth}` | Owner-scoped flow; no authorization code, verifier, token or secret in result/log. |
| `ai_auth_cancel` | `{operationId,authId:UUID}` | `{state:"cancelled"\|"already-terminal"}` | Retire callback/state and discard late completion. No effect on unrelated stored credential. |
| `ai_auth_secret_set` | `{operationId,providerId:string<=64,expectedCredentialId:string|null,secret:string[1..8192]}` | `{auth:PublicAuth}` | Only for an adapter's supported API-key method; native protected storage, never SQLite preference/workspace/chat/log. Secret is write-only; disable argument retention/logging, including operation ledger payloads. Never echo it in error/replay. Missing secure store fails closed. |
| `ai_auth_forget` | `{operationId,providerId:string<=64,expectedCredentialId:string}` | `{auth:PublicAuth,state:"forgotten"}` | Exact opaque credential fence; cancel matching requests/refresh and clear local native secret. Report remote revocation separately when adapter supports it; local deletion success must not falsely claim remote revoke. |

Provider entry configuration is a strict discriminated union, covering #170/#174 scope:

```ts
type ProviderEntry = {
  id: string; name: string; // ID <=64, label 1..120
  connection:
    | {kind:"openai-compatible"; baseUrl:string; credentialId:string|null}
    | {kind:"ollama"; baseUrl:string}
    | {kind:"acp"; installedEntryId:string}
    | {kind:"subscription-oauth"; adapterId:string; credentialId:string|null};
};
```

`baseUrl` is at most 2048 characters, parsed/canonicalized by native code, and has no username/password, fragment or embedded secret. OpenAI-compatible supports configured API-key endpoints over HTTPS; HTTP requires explicit local endpoint classification, not silent fallback. Ollama supports explicit local HTTP/HTTPS endpoint configuration, including the native local default, without inventing API-key auth. Both persist endpoint intent; neither accepts shell commands. ACP accepts only an installed restricted entry ID from `ai_acp_entries_list({limit:int[1,64],cursor?:string}) -> {entries:[{id,label,capabilities,configurationRevision}],nextCursor}`. It cannot supply executable paths, argv, environment variables or alter the installed entry's restriction policy. Subscription OAuth uses a shipped supported account adapter with native browser login; callers cannot select arbitrary OAuth authorization/token URLs or scopes. Subscription access and model availability are reported from that adapter, not assumed from an API key or another provider's login.

| Proposed provider task | Strict input | Success data | Boundaries and negative outcomes |
| --- | --- | --- | --- |
| `ai_provider_add` | `{operationId,expectedSettingsRevision:Revision,entry:ProviderEntry}` | `{settingsRevision,provider:PublicProvider}` | Max 16 entries; ID unique. Creates saved configuration only, no login, connectivity test, chat or selection. Unknown kind, invalid URL, inaccessible restricted ACP entry or unsupported subscription adapter rejects. New entry may explicitly report missing authentication. |
| `ai_provider_update` | `{operationId,providerId,expectedProviderRevision:Revision,connection:ProviderEntry.connection,name:string[1..120]}` | `{providerRevision,provider:PublicProvider,testState:"not-tested"}` | Exact entry revision; changed endpoint/credential/ACP configuration invalidates prior test result. Reject while an owned auth configuration transition is active, or require cancellation first. Never carry an old key across a changed endpoint without explicit credential selection. |
| `ai_provider_test` | `{operationId,providerId,expectedProviderRevision:Revision}` | `{testId:UUID,state:"running"}` | Native bounded handshake/model discovery only, no chat/tool execution. Resolve the exact endpoint/key, local Ollama endpoint, restricted ACP entry or subscription session. No credentials in diagnostics. |
| `ai_provider_test_status` | `{testId:UUID}` | `{state:"running"\|"passed"\|"failed"\|"cancelled",providerRevision,checkedAt,models:PublicModel[0..64],nextCursor:string|null,error:PublicError|null}` | 15 second connection/30 second overall deadline, <=64 KiB diagnostics. Distinguish DNS/TLS/refused connection, incompatible API, HTTP auth rejection, missing key, missing Ollama model, unavailable/restricted ACP entry, expired subscription and unsupported model. A test for an older revision cannot mark the new revision passed. |
| `ai_provider_test_cancel` | `{operationId,testId:UUID}` | `{state:"cancelled"\|"already-terminal"}` | Exact owner/test cancellation; discard late completion; no saved entry change. |
| `ai_provider_select` | `{operationId,expectedSettingsRevision:Revision,providerId,expectedProviderRevision:Revision,modelId:string[1..128]}` | `{settingsRevision,selected:{providerId,providerRevision,modelId},readiness}` | Select only a configured entry and model supported by its current discovered capability contract. Return unavailable readiness for expired auth/unreachable service; do not silently select a different provider/model or start chat. |
| `ai_provider_remove` | `{operationId,providerId,expectedProviderRevision:Revision,expectedSettingsRevision:Revision}` | `{settingsRevision,removedId,selected:null\|{providerId,modelId}}` | Reject active request/auth/test with `provider_busy` and identifiers to cancel; no surprise cancellation or fallback. Removing selected entry clears selection. Clear entry-owned native credential references and revoke locally owned credentials only when no other entry references them; report remote account revocation separately. |

Each kind declares supported auth/capability methods. `ai_auth_secret_set` applies to OpenAI-compatible API keys; OAuth begins only for supported subscription adapters. ACP uses its installed restricted entry's supported login flow, never a generic arbitrary-process launch. Credentials remain native and never enter model context or external MCP resource reads. Public state can be observed through `settings.ai` without auth-flow secrets. At most one auth flow and one connectivity test/provider; 5 minute auth flow TTL (or shorter provider TTL); 16 entries and 64 KiB aggregate public settings. Tests cover all four add/test/select/update/remove flows, stale settings/provider revisions, denied/locked store, failed/expired login, post-cancel completion, endpoint credential isolation, unavailable local model/ACP entry, secret redaction and crash/restart.

## Planned chat and tool approval

Use the shared execution doc's one native task owner for manual UI, embedded chat and MCP operations. Provider model responses are untrusted proposals. Do not directly translate arbitrary tool names/JSON into native invocation. The allowlisted explicit task schema remains the execution boundary.

| Proposed task | Strict input | Success data | Boundaries and negative outcomes |
| --- | --- | --- | --- |
| `ai_chat_create` | `{operationId,title?:string[1..120]}` | `{chatId:UUID,revision:Revision}` | Persist only bounded chat metadata; no automatic context collection or provider call. |
| `ai_chat_list` | `{cursor?:string<=256,limit:int[1,64]}` | `{chats:ChatSummary[],nextCursor:string|null}` | Metadata only; owner/access checks. |
| `ai_chat_read` | `{chatId:UUID,afterSequence:int>=0,limit:int[1,100]}` | `{revision,events:ChatEvent[],nextSequence:int,hasMore:boolean}` | Bounded redacted history; max 256 KiB reply. Tool summaries retain submitted/accepted/completed distinctions and actual result identity. |
| `ai_chat_send` | `{operationId,chatId:UUID,expectedRevision:Revision,message:string[1..32768],context:ContextRef[0..16]}` | `{requestId:UUID,messageId:UUID,state:"queued",revision}` | ContextRef is `{kind:"profile"\|"widget"\|"panel"\|"activity",id:string<=64,revisionOrHash:string<=128}`; resolve only explicit permitted snapshots, <=256 KiB total. Reject stale/missing context, unreadable auth, capacity, concurrent request/chat, invalid model config. No raw local paths or secret/workspace-wide collection. |
| `ai_chat_cancel` | `{operationId,requestId:UUID}` | `{state:"cancelled"\|"already-terminal",nativeOperationsStillRunning:UUID[]}` | Cancel exact provider stream and retire unapproved proposals; preserve partial transcript marked cancelled. Already-submitted native work follows its actual task cancellation/cleanup contract, never falsely reported undone. |
| `ai_approval_read` | `{approvalId:UUID}` | `{state,chatId,requestId,taskName,argumentsRedacted,argumentsHash:Hash,preconditions:Precondition[],effects:EffectSummary[],expiresAt}` | Show exact executable proposal, revision/owner/device fences and consequences. Redaction must not conceal an effect needed for approval; secret-bearing tasks use a dedicated native flow. |
| Trusted native human approval action (not an MCP tool) | `{approvalId:UUID,decision:"approve"\|"reject",expectedArgumentsHash:Hash}` through authenticated local human UI event | `{state:"rejected"}\|{state:"submitted",nativeOperationId:UUID}` | Approval scoped to one exact task/arguments and expires in 5 minutes. Native verifies trusted human interaction, then rechecks current revisions, owner and capabilities. Changed arguments, stale owner/revision, expiry, disconnected target or unsupported task refuses without creating a native operation. External callers can inspect approval state but cannot decide it. |

Bound proposed chat storage to 100 chats, 1,000 events/chat, 32 MiB aggregate UTF-8; fail explicitly rather than silently deleting history. Bound to 1 active request/chat, 2/client, 8 globally; provider connect deadline 15 seconds, idle stream deadline 60 seconds and overall request deadline 5 minutes. Retain request/approval IDs across reconnect; after app restart mark incomplete provider requests interrupted and approvals expired, then reconcile native operation IDs. New provider requests must not automatically replay a submitted mutation.

Apply the existing shared task's approval policy; this document does not add blanket approvals to all writes. When a task requires human approval, only the trusted local human interaction can decide it. No external MCP privilege, credential scope, provider output or agent-created token can approve or mint an approval capability. External clients receive `awaiting-human-approval` plus inspectable proposal state and wait for the observed native decision. Test prompt-injected task names, modified argument hashes, approval replay or external decision attempts, stale native fences, cancel during stream and after native submission, restart reconciliation, and leak checks for credentials and unselected context. Chat deletion is outside this proposed contract's scope.

## Availability, not absence

- Mobile: `crates/companion-service/src/client.rs:795-822` derives authentication availability from protected-store status and the native platform. Android/iOS auth is refused unless both debug assertions and feature `mobile-qualification` are enabled. The reason explicitly names native protected-store and lifecycle qualification (#55/#56); local preview and protected-store probing remain available. Locked/missing store and saved credential open failures are separate native errors. Local Network denial also has a separate `check_network_access` error. Host `companion_pair_offer(kind:"mobile")` existing does not prove iOS authentication works or a physical phone is qualified.
- ESP32: `src-tauri/src/companion/host/commands.rs:21-30` explicitly pauses new pairing while its replacement renderer is deferred. It refuses an ESP32 offer and provisioning without a nonempty paired companionId, with “ESP32 pairing is temporarily unavailable.” Existing paired screens retain their connection-settings flow. Approved firmware update, candidate validation and physical USB absence are separate states, not evidence that MCP tasks are missing.
- Generic compiled designer: `CompiledCompanionDesigner.svelte` labels Browser target and disables assignment choices whose client kind is not browser. `companion/storage/widgets.rs` guide explicitly says ESP32 remains outside this feature. Having firmware/USB tools does not add compiled ESP32 support. Record unsupported target/gate separately from absent compile or simulation API.

Current behavioral references: `tests/ui/generic-widget-designer-native.mjs`, `mcp-widgets-native.mjs`, `mcp-designer-observation.mjs`, `sqlite-designer-native.mjs`, `desktop-autostart-observation-native.mjs`, `companion-firmware-update-browser.mjs`, and native `companion/storage/workspace_tests.rs`, `widget_tests.rs`, `companion/host/tests/upload_validation.rs`, plus client authentication tests in `crates/companion-service/src/client.rs`. These are source references; this audit did not execute or claim physical qualification.
