use super::*;
#[path = "companion_catalogue.rs"]
mod companion;

fn string(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max,"pattern":"^[^\\u0000-\\u001f\\u007f]*$"})
}
fn game_id() -> Value {
    json!({"type":"string","format":"game-id","minLength":1,"maxLength":96,"pattern":"^[a-z0-9][a-z0-9-]{0,95}$"})
}
fn integer(min: u64, max: u64) -> Value {
    json!({"type":"integer","minimum":min,"maximum":max})
}
fn choice(values: &[&str]) -> Value {
    json!({"type":"string","enum":values})
}
fn object() -> Value {
    json!({"type":"object"})
}
fn nullable_revision() -> Value {
    json!({"type":["integer","null"],"minimum":1,"maximum":osg_profiles::MAX_REVISION})
}
fn revision() -> Value {
    integer(1, osg_profiles::MAX_REVISION)
}
fn owner() -> Value {
    json!({"type":["object","null"],"description":"Exact profileSession.owner from profile_status, or explicit null when stopped. Never invent a native owner.","additionalProperties":false,"properties":{"session":string(36),"generation":revision(),"profileId":string(36),"profileRevision":revision(),"gameId":{"type":["string","null"]}},"required":["session","generation","profileId","profileRevision","gameId"]})
}
fn scope() -> Value {
    choice(&[
        "game-provider",
        "desktop",
        "selection-mode",
        "game-rule",
        "game-preference",
    ])
}
fn page() -> Vec<(&'static str, Value)> {
    vec![("offset", integer(0, 262_144)), ("limit", integer(1, 64))]
}
fn fields(
    mut base: Vec<(&'static str, Value)>,
    extra: Vec<(&'static str, Value)>,
) -> Vec<(&'static str, Value)> {
    base.extend(extra);
    base
}
fn tool(name: &str, description: &str, properties: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let mutation = is_mutation(name);
    let mut props = properties
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect::<serde_json::Map<String, Value>>();
    let mut required = required.iter().map(|key| json!(key)).collect::<Vec<_>>();
    if mutation {
        props.insert("operationId".into(),json!({"type":"string","format":"uuid","minLength":36,"maxLength":36,"description":"Fresh canonical lowercase UUID. Replay the exact same arguments with this identity to recover the original result; never reuse for different work."}));
        required.push(json!("operationId"));
    }
    let mut input = json!({"type":"object","properties":props,"required":required,"additionalProperties":false});
    if matches!(name, "profile_prepare_changes" | "profile_import") {
        input["$defs"] = schema::definitions();
    }
    json!({"name":name,"description":description,"inputSchema":input,"outputSchema":result_schema(),"annotations":{"readOnlyHint":!mutation,"destructiveHint":mutation,"idempotentHint":true,"openWorldHint":false}})
}

/// Explicit discoverable tasks; there is intentionally no generic invoke or
/// gateway command passthrough, even for an authenticated write-enabled client.
pub fn catalogue() -> Vec<Value> {
    let mut tools=vec![
        tool("storage_status","List native SQLite backups and the exact catalogue revision. Backup paths and arbitrary SQL are never accepted.",vec![],&[]),
        tool("storage_create","Create an online SQLite backup with hash-checked immutable assets. One backup slot is reserved for restore rollback.",vec![("expectedRevision",integer(0,9007199254740990))],&["expectedRevision"]),
        tool("storage_remove","Retire one exact saved backup then clean up its private immutable bundle.",vec![("id",string(36)),("expectedRevision",integer(0,9007199254740990))],&["id","expectedRevision"]),
        tool("storage_restore","Verify a saved backup, require stopped profiles and completed recovery, stop native workers and queue restore for the next restart. This replaces settings/content and invalidates all historical credentials and cleanup authority. Explicit confirmed:true is required.",vec![("id",string(36)),("expectedRevision",integer(0,9007199254740990)),("confirmed",json!({"type":"boolean","const":true}))],&["id","expectedRevision","confirmed"]),
        tool("storage_restart","Restart the native Cockpit after a restore was explicitly queued. Reconnect and reissue credentials after startup.",vec![],&[]),
        tool("ui_preferences_get","Read native SQLite UI preferences and revisions. Omit key for the seven frontend owners, or choose theme, devices-mode, canvas-navigation, canvas-styling, remembered, tray, progress or windows. Remembered observations remain stale/unowned until providers refresh.",vec![("key",choice(&["theme","devices-mode","canvas-navigation","canvas-styling","remembered","tray","progress","windows"]))],&[]),
        tool("ui_preferences_save","Save one exact native UI settings revision. Values: theme boolean; devices-mode list/topology; canvas-navigation {wheelAction:pan/zoom}; canvas-styling {arrowStyle:smooth/orthogonal,cornerRadius:0..100}; tray {closeToTray:boolean,minimizeToTray:boolean}; progress {delivery:system/window,position:bottom-right/bottom-left/top-right/top-left}; windows {x,y,width,height,scaleFactor,maximized} for next startup; remembered {components,forgotten} is bounded metadata, never native device authority. Native validators reject extra fields. Actual device Forget/revoke and OS autostart use their own guarded controls.",vec![("key",choice(&["theme","devices-mode","canvas-navigation","canvas-styling","remembered","tray","progress","windows"])),("expectedRevision",integer(0,9007199254740990)),("value",json!({"type":["boolean","string","object","null"]}))],&["key","expectedRevision","value"]),
        tool("operation_status","Reconcile a retained mutation for this authenticated client. The ledger survives reconnects during this MCP server lifetime; reconcile native state after a server restart.",vec![("operationId",string(36))],&["operationId"]),
        tool("profiles_list","List saved profile summaries, revisions, eligibility and selection. Saving and selection do not activate outputs.",page(),&[]),
        tool("profile_get","Read a saved portable profile and its exact machine-local resource assignments. Use returned revision for edits.",vec![("id",string(36))],&["id"]),
        tool("profile_export","Export one portable JSON profile. Machine-local assignments are deliberately separate.",vec![("id",string(36))],&["id"]),
        tool("profile_status","Inspect selected, saved and applied snapshots, exact native owner, recovery and automatic policy. This never starts outputs.",vec![],&[]),
        tool("profile_create","Create an empty saved profile without changing the applied run. Prefer profile_prepare_changes for validated authoring.",vec![("name",string(80))],&["name"]),
        tool("profile_duplicate","Duplicate an exact saved revision to a new identity without activating it. Reconnect retries require the original operationId.",vec![("id",string(36)),("name",string(80)),("expectedRevision",revision())],&["id","name","expectedRevision"]),
        tool("profile_rename","Rename exactly one saved revision. A stale expectedRevision is rejected; no activation is implied.",vec![("id",string(36)),("name",string(80)),("expectedRevision",revision())],&["id","name","expectedRevision"]),
        tool("profile_delete","Delete exactly one saved revision. The default profile and active ownership are protected by native admission.",vec![("id",string(36)),("expectedRevision",revision())],&["id","expectedRevision"]),
        tool("profile_import","Validate and import a portable profile into a fresh identity. Unknown nested fields, oversized graphs and invalid bindings are rejected.",vec![("document",schema::reference("ProfileDocument")),("name",string(80))],&["document"]),
        tool("resources_list","List cached native devices, modules, controllers, games and companions. Retained offline identities are distinguished from current capabilities.",page(),&[]),
        tool("resource_inspect","Inspect current/retained native port descriptors, contracts and stable identity evidence. These metadata do not grant output ownership.",fields(vec![("resourceId",string(36))],page()),&["resourceId"]),
        tool("resource_search_ports","Search command/telemetry/action port metadata within one exact resource, returning a bounded matching page. Refresh after capabilityStamp changes.",fields(vec![("resourceId",string(36)),("search",string(200))],page()),&["resourceId","search"]),
        tool("processing_operations_list","Discover/search native processing operations and their exact typed parameter schemas before adding a ProcessingNode. Returns stable schema hashes and bounded metadata; the shared native compiler decides compatibility with declared input/output port contracts. This is passive metadata and executes no operation.",fields(vec![("search",json!({"type":"string","maxLength":200})),("category",choice(&["numeric","logic","event","text","action","quantity","script","other"]))],page()),&[]),
        tool("games_list","List discovered games with their tagIds, provider readiness, setup revisions and current vehicle context. This does not launch a game.",page(),&[]),
        tool("game_discovery_start","Start asynchronous local game discovery. Returns the current scan ID and snapshot immediately; use game_discovery_status to poll and game_discovery_apply to commit explicitly selected Add/Update actions. No catalogue changes occur before Apply.",vec![("paths",json!({"type":"array","maxItems":64,"items":{"type":"string","minLength":1,"maxLength":4096}}))],&[]),
        tool("game_discovery_status","Read progress, discovered entries and diagnostics for one exact discovery scan. Results are paged; Add/Update entries default selected and Remove entries default unselected.",fields(vec![("scanId",string(36))],page()),&["scanId"]),
        tool("game_discovery_apply","Atomically apply selected discovery Add/Update/Remove actions after reviewing the exact scan. Native code revalidates the scan and installation evidence; stale scans fail without partial changes.",vec![("scanId",string(36)),("actions",json!({"type":"array","maxItems":512,"items":{"type":"object","additionalProperties":false,"properties":{"id":string(256),"selected":{"type":"boolean"},"executable":{"type":["string","null"],"maxLength":4096},"replaceInstallationId":{"type":["string","null"],"maxLength":256},"gameId":game_id()},"required":["id","selected"]}}))],&["scanId","actions"]),
        tool("game_artwork_list","List cached artwork for the exact library identities attached to one imported game. Returns thumbnail images and opaque IDs; use game_artwork_preview before aligning and saving an image.",fields(vec![("gameId",game_id())],page()),&["gameId"]),
        tool("game_artwork_preview","Load a full bounded image for an artwork ID returned by game_artwork_list. No saved image changes until game_background_save.",vec![("gameId",game_id()),("artworkId",string(64))],&["gameId","artworkId"]),
        tool("game_background_save","Save a game's image and normalized 16:9 crop from game_artwork_preview, or reset with path:null. Returns a bounded receipt. This does not launch a game.",vec![("gameId",game_id()),("imageData",string(174792)),("crop",json!({"type":"object","additionalProperties":false,"properties":{"x":{"type":"number","minimum":0,"maximum":1},"y":{"type":"number","minimum":0,"maximum":1},"width":{"type":"number","exclusiveMinimum":0,"maximum":1},"height":{"type":"number","exclusiveMinimum":0,"maximum":1}},"required":["x","y","width","height"]})),("path",json!({"type":["string","null"],"maxLength":4096})),("positionX",json!({"type":"number","minimum":0,"maximum":100})),("positionY",json!({"type":"number","minimum":0,"maximum":100}))],&["gameId"]),
        tool("game_tags_list","List the shared game tag taxonomy without changing game metadata.",vec![],&[]),
        tool("game_tag_definitions_save","Replace the complete game tag taxonomy using exact expectedTags for optimistic concurrency. Tag IDs use the generic game ID syntax; names are 1–48 printable characters; at most 128 tags are allowed.",vec![("tags",json!({"type":"object","maxProperties":128,"propertyNames":{"type":"string","format":"game-id"},"additionalProperties":{"type":"string","minLength":1,"maxLength":48}})),("expectedTags",json!({"type":"object","maxProperties":128,"propertyNames":{"type":"string","format":"game-id"},"additionalProperties":{"type":"string","minLength":1,"maxLength":48}}))],&["tags","expectedTags"]),
        tool("game_tags_save","Replace one game's complete tag ID list using exact expectedTagIds for optimistic concurrency. Tag IDs use the generic game ID syntax; at most 32 unique IDs are allowed.",vec![("gameId",game_id()),("tagIds",json!({"type":"array","maxItems":32,"uniqueItems":true,"items":game_id()})),("expectedTagIds",json!({"type":"array","maxItems":32,"uniqueItems":true,"items":game_id()}))],&["gameId","tagIds","expectedTagIds"]),
        tool("game_save","Update metadata or selected installation for one already discovered game. This does not launch a game or start a provider.",vec![("gameId",game_id()),("name",string(128)),("installationId",string(256)),("executable",json!({"type":["string","null"],"maxLength":4096}))],&["gameId"]),
        tool("game_remove","Remove one registered game and its installations from the local catalogue.",vec![("gameId",game_id())],&["gameId"]),
        tool("game_installation_select","Select one discovered installation for a registered game without launching it.",vec![("gameId",game_id()),("installationId",string(256))],&["gameId","installationId"]),
        tool("game_vehicles_list","Search/page one game's retained native aircraft/car catalogue without selecting a vehicle, registering resources or starting a provider. Includes current native vehicle context and a catalogue/setup stamp; these metadata do not prove current live readiness.",fields(vec![("gameId",game_id()),("search",json!({"type":"string","maxLength":200}))],page()),&["gameId"]),
        tool("game_telemetry_inspect","Read bounded passive telemetry values and canonical channel metadata from one game. This does not start a provider or apply a profile.",fields(vec![("gameId",string(64)),("search",json!({"type":"string","maxLength":200})),("group",json!({"type":"string","maxLength":256}))],page()),&[]),
        tool("game_aircraft_resource","Explicitly register an installed DCS aircraft catalogue as an authoring resource in native inventory. This changes catalogue discovery but does not activate a game/provider/profile. Uses native installed catalogue validation; accepts no filesystem path.",vec![("vehicleId",string(128))],&["vehicleId"]),
        tool("companions_list","List retained companion resources with exact identities and connection state from the native inventory.",page(),&[]),
        tool("mcp_settings_get","Read this running Cockpit MCP server's public configuration, credential metadata and exact settingsRevision. Bearer tokens and hashes are excluded.",vec![],&[]),
        tool("mcp_settings_update","Save local MCP enablement/port at exact expectedSettingsRevision. Disabling or changing port requires reconnecting to the returned endpoint.",vec![("enabled",json!({"type":"boolean"})),("port",integer(1024,65535)),("expectedSettingsRevision",integer(0,9007199254740991))],&["enabled","port","expectedSettingsRevision"]),
        tool("mcp_credential_create","Create one named read/write credential at the exact settings revision. The reply contains its one-time token; Cockpit persists only its hash. Configure the intended client with that token.",vec![("name",string(80)),("scope",choice(&["read","write"])),("expectedSettingsRevision",integer(0,9007199254740991))],&["name","scope","expectedSettingsRevision"]),
        tool("mcp_credential_rotate","Atomically replace a credential at exact expectedSettingsRevision, retaining its name/scope and returning the one-time replacement token. The old token is revoked in the same SQLite commit.",vec![("id",string(36)),("expectedSettingsRevision",integer(0,9007199254740991))],&["id","expectedSettingsRevision"]),
        tool("mcp_credential_revoke","Revoke one credential at exact expectedSettingsRevision. Its token immediately loses access; other credentials are preserved.",vec![("id",string(36)),("expectedSettingsRevision",integer(0,9007199254740991))],&["id","expectedSettingsRevision"]),
        tool("companion_listener_get","Read native listener configuration, configured/live state, errors and exact settingsRevision. This uses the same native companion setup as the portal.",vec![],&[]),
        tool("companion_listener_save","Enable or disable the native companion listener at exact expectedSettingsRevision. Enabling requires address IP, port, advertisedHost and discovery. Configuration commits in SQLite before listener startup; bind failures retain configured intent and report the live error. Disable commits before stopping; rejected writes preserve the prior setting.",vec![("enabled",json!({"type":"boolean"})),("expectedSettingsRevision",integer(0,9007199254740991)),("address",string(64)),("port",integer(1024,65535)),("advertisedHost",string(253)),("discovery",json!({"type":"boolean"}))],&["enabled","expectedSettingsRevision"]),
        tool("companion_panels_list","List retained verified companion panel publications and compatibility; publication hashes are immutable authoring metadata.",page(),&[]),
        tool("designer_workspace_read","Read the native designer workspace as UTF-8 JSON in 48 KiB base64 chunks. Preserve revision and workspaceHash; pass expectedHash for later offsets to reject concurrent edits. Null JSON means no saved workspace. Drafts, source revisions and verified publications share one native SQLite authority.",vec![("offset",integer(0,67108864)),("expectedHash",string(71))],&[]),
        tool("designer_workspace_begin","Begin a bounded designer snapshot upload. Read current workspace first; edit its plain sources/drafts/publications. Provide byte length and sha256 hash of the complete UTF-8 JSON. Nothing is persisted until commit; uploads expire after ten minutes.",vec![("expectedRevision",integer(0,9007199254740991)),("expectedLibraryRevision",integer(0,9007199254740991)),("totalBytes",integer(1,67108864)),("workspaceHash",string(71))],&["expectedRevision","expectedLibraryRevision","totalBytes","workspaceHash"]),
        tool("designer_workspace_chunk","Append at most 48 KiB of UTF-8 JSON bytes, base64 encoded, to your exact uploadId at the acknowledged byte offset.",vec![("uploadId",string(36)),("offset",integer(0,67108864)),("base64",string(65536))],&["uploadId","offset","base64"]),
        tool("designer_workspace_commit","Validate and atomically save the complete designer workspace plus immutable shared widget sources at expectedRevision. Rejected or busy writes preserve current data and keep the upload for retry. Saving authoring data does not assign or activate outputs.",vec![("uploadId",string(36))],&["uploadId"]),
        tool("designer_workspace_abort","Discard your incomplete designer upload without changing saved state.",vec![("uploadId",string(36))],&["uploadId"]),
        tool("widgets_list","List generic widget sources in the shared Cockpit Widgets library. Returns the latest source revision; Toggle is the sole built-in seed.",page(),&[]),
        tool("widget_get","Read the current plain widget source by ID. Artwork, states, signal expressions and effects compile into finished pixels.",vec![("id",string(64))],&["id"]),
        tool("widget_save","Save one current widget source in place. Read cockpit://guide/widgets. Send {definitions:[source]} and the exact expectedRevision from widgets_list/widget_get. Sources have no format or revision fields. Native admission checks the complete source. This does not compile, publish, assign or activate a panel.",vec![("bundle",object()),("expectedRevision",integer(0,9007199254740990))],&["bundle","expectedRevision"]),
        tool("companion_inspect","Inspect a retained companion panel's typed ports using an exact native resource identity and immutable publication hash.",fields(vec![("resourceId",string(36)),("publicationHash",string(71))],page()),&["resourceId","publicationHash"]),
        tool("profile_prepare_changes","Prepare and compile an ephemeral draft without saving, selecting or activating. Supply either profileId plus expectedRevision and typed edits; a full document plus explicit null/numeric expectedRevision; or name plus expectedRevision:null for a new graph. Local assignments remain separate. Returns draftId, deterministic diff, validation and native compiler diagnostics. Read cockpit://schemas/profile and cockpit://guide/configuration first.",vec![("profileId",string(36)),("expectedRevision",nullable_revision()),("name",string(80)),("document",schema::reference("ProfileDocument")),("edits",json!({"type":"array","maxItems":256,"items":schema::reference("Edit")})),("assignments",json!({"type":"array","maxItems":64,"items":schema::reference("LocalAssignment")}))],&["expectedRevision"]),
        tool("profile_save_prepared","Atomically persist the exact prepared document and local assignments with its captured expectedRevision. A save never applies or reapplies outputs. Drafts expire after 30 minutes and belong to this client.",vec![("draftId",string(36))],&["draftId"]),
        tool("compilation_inspect","Page diagnostics or compiled trace from a recent native preview. Cached previews never authorize Apply; fresh native admission is mandatory.",fields(vec![("compilationId",string(36)),("section",choice(&["diagnostics","nodes","order","sources","sinks","branches","controllers","visibility"]))],page()),&["compilationId","section"]),
        tool("profile_apply","Explicitly activate one exact saved revision. Requires observed expectedActiveOwner (including explicit null) and fresh native resource admission. operationId also identifies activation progress. Save remains separate.",vec![("id",string(36)),("expectedRevision",revision()),("expectedActiveOwner",owner()),("gameId",game_id())],&["id","expectedRevision","expectedActiveOwner"]),
        tool("activation_status","Inspect exact native activation progress, cancellation capability and outcome without waiting for admission locks.",vec![("activationId",string(36))],&["activationId"]),
        tool("activation_current","Inspect current native activation or automatic switch progress. Null means no retained current operation.",vec![],&[]),
        tool("activation_cancel","Cancel an exact pending native activation. Its epoch fence cannot deactivate a newer applied owner. Cleanup is never cancelled.",vec![("activationId",string(36))],&["activationId"]),
        tool("profile_stop","Explicitly stop and neutralize native profile outputs. Emergency cleanup stays available when preparation fails or another operation is pending.",vec![],&[]),
        tool("profile_recover","Explicitly retry cleanup of retained native ownership/recovery receipts. Does not replay a saved graph or activate outputs.",vec![],&[]),
        tool("applied_ports_inspect","Inspect actual declared-port samples for an exact applied owner. Saved/selected profile state cannot impersonate an applied runtime.",fields(vec![("owner",owner()),("nodeId",string(80))],page()),&["owner"]),
        tool("applied_topology_inspect","Page immutable nodes or edges from an exact native applied owner.",fields(vec![("owner",owner()),("section",choice(&["nodes","edges"]))],page()),&["owner","section"]),
        tool("system_inventory","Read supplemental native virtual/hidden/controller inventory. Reuse observedAt when paging a captured snapshot.",vec![("offset",integer(0,4096)),("limit",integer(1,32)),("observedAt",integer(1,u64::MAX))],&[]),
        tool("gateways_list","Discover supported gateway identities and ports without opening endpoints. Use the exact gatewayId for every subsequent operation.",page(),&[]),
        tool("gateway_inspect","Read one connected/discovered gateway state and typed module capabilities.",vec![("gatewayId",string(100))],&["gatewayId"]),
        tool("gateway_session","Connect or disconnect an exact observed gateway through shared native profile admission. Independent setup cannot take over an applied profile.",vec![("gatewayId",string(100)),("action",choice(&["connect","disconnect"])),("expectedActiveOwner",owner())],&["gatewayId","action","expectedActiveOwner"]),
        tool("gateway_config_read","Read the complete native module configuration and its current boot/route state before opening a setup session.",vec![("gatewayId",string(100)),("moduleId",string(80))],&["gatewayId","moduleId"]),
        tool("gateway_config_change","Perform one explicit typed module setup action through the native claim/lease route. Apply requires config; persistence requires apply_save or save. Native boot and ownership checks remain authoritative.",vec![("gatewayId",string(100)),("moduleId",string(80)),("action",choice(&["claim","release","apply","apply_save","save","discard","reset"])),("config",schema::runtime_config()),("expectedActiveOwner",owner())],&["gatewayId","moduleId","action","expectedActiveOwner"]),
        tool("usb_modules_list","List bounded direct USB module identities, connection state and admissibility without changing setup ownership.",vec![("offset",integer(0,128)),("limit",integer(1,32))],&[]),
        tool("usb_module_inspect","Inspect one exact native USB module key and its current route token.",vec![("key",string(256))],&["key"]),
        tool("usb_module_session","Connect/disconnect a currently observed direct USB endpoint. Connect requires its discovered port and key; native admission checks exact profile ownership.",vec![("key",string(256)),("port",string(256)),("action",choice(&["connect","disconnect"])),("expectedActiveOwner",owner())],&["key","action","expectedActiveOwner"]),
        tool("usb_module_config_read","Read configuration using an exact current native USB route token.",vec![("key",string(256)),("route",object())],&["key","route"]),
        tool("usb_module_config_change","Perform a typed direct USB module setup action with its exact current native route, client claim and profile owner. Stale boot/route tokens fail closed.",vec![("key",string(256)),("route",object()),("action",choice(&["claim","release","apply","apply_save","save","discard","reset"])),("config",schema::runtime_config()),("expectedActiveOwner",owner())],&["key","route","action","expectedActiveOwner"]),
        tool("device_config_prepare","Read one exact gateway/USB module configuration, validate a complete candidate with the native config encoder, and return a deterministic reviewable diff. Performs no claim, apply or save. Gateway selectors are gatewayId+moduleId; USB selectors are key+current route. The resulting baseline is review context; explicit setup rechecks native ownership, boot and route, and firmware semantic acceptance remains an Apply result.",vec![("transport",choice(&["gateway","usb"])),("gatewayId",string(100)),("moduleId",string(80)),("key",string(256)),("route",object()),("config",schema::runtime_config())],&["transport","config"]),
        tool("local_settings_get","Read revisioned machine-local game-provider, desktop or automatic-selection preferences. Path-bearing provider preferences never enter a portable profile.",vec![("scope",scope()),("gameId",game_id())],&["scope"]),
        tool("local_settings_save","Save typed machine-local preferences with the exact settings revision and observed active owner. Scope-specific native validation rejects unknown fields; provider paths are preferences, never arbitrary file access. Read the workflow resource for scope settings.",vec![("scope",scope()),("gameId",game_id()),("expectedSettingsRevision",string(128)),("expectedActiveOwner",owner()),("settings",object())],&["scope","expectedSettingsRevision","expectedActiveOwner","settings"]),
    ];
    tools.extend(companion::catalogue());
    tools
}

pub fn is_mutation(name: &str) -> bool {
    super::companion_controls::mutation(name)
        || matches!(
            name,
            "storage_create"
                | "storage_remove"
                | "storage_restore"
                | "storage_restart"
                | "profile_create"
                | "ui_preferences_save"
                | "widget_save"
                | "mcp_settings_update"
                | "mcp_credential_create"
                | "mcp_credential_rotate"
                | "mcp_credential_revoke"
                | "companion_listener_save"
                | "designer_workspace_begin"
                | "designer_workspace_chunk"
                | "designer_workspace_commit"
                | "designer_workspace_abort"
                | "profile_duplicate"
                | "profile_rename"
                | "profile_delete"
                | "profile_import"
                | "profile_save_prepared"
                | "profile_apply"
                | "activation_cancel"
                | "profile_stop"
                | "profile_recover"
                | "game_discovery_apply"
                | "game_discovery_start"
                | "game_background_save"
                | "game_save"
                | "game_tag_definitions_save"
                | "game_tags_save"
                | "game_remove"
                | "game_installation_select"
                | "gateway_session"
                | "gateway_config_change"
                | "usb_module_session"
                | "usb_module_config_change"
                | "local_settings_save"
                | "game_aircraft_resource"
        )
}

pub fn result_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{"ok":{"type":"boolean"},"data":{},"operationId":{"type":"string"},"error":{"type":"object","additionalProperties":false,"properties":{"code":{"type":"string"},"message":{"type":"string"},"retryable":{"type":"boolean"},"guidance":{"type":"string"}},"required":["code","message","retryable","guidance"]}},"required":["ok"]})
}
pub fn resources() -> Vec<Value> {
    [
        ("cockpit://guide/configuration","Cockpit configuration workflow","Saved, prepared and applied state; tool sequencing; revision and ownership rules."),
        ("cockpit://schemas/profile","Portable profile contract","Canonical empty profile and typed edits; the native parser rejects unknown nested fields."),
        ("cockpit://schemas/local-assignment","Local assignment contract","Exact native source and port pins, stored separately from portable graphs."),
        ("cockpit://schemas/task-results","Task result contract","Structured success/error results and retry/reconciliation semantics."),
        ("cockpit://status","Current Cockpit profile status","Live selected/saved/applied native status, owner and recovery state."),
        ("cockpit://guide/widgets","Portable widget creation","Shared widget library, source format, immutable revisions, examples and save workflow."),
    ].into_iter().map(|(uri,name,description)|json!({"uri":uri,"name":name,"description":description,"mimeType":"application/json"})).collect()
}
pub fn workflow() -> Value {
    json!({"schemaVersion":1,"workflow":["Inspect profile_status, profiles_list, resources_list and the exact resource ports. Read installed game/provider and retained companion metadata as needed.","Read profile_get for an existing document, local assignments and saved revision. Never confuse selection or a saved draft with the applied snapshot.","Call profile_prepare_changes with explicit expectedRevision and full document or typed edits. It compiles a bounded ephemeral draft and returns a deterministic diff without writing or activating.","Read compilation_inspect diagnostics and repair blocking authoring mistakes. Hardware/offline readiness remains visible in the native report.","Call profile_save_prepared with draftId and a fresh operationId. One atomic native transaction persists the graph and local assignments without reapplying the running revision.","Activation is a separate explicit profile_apply with saved revision and exact observed expectedActiveOwner or null. Poll activation_status using its operationId; cancellation only revokes that activation.","Use profile_stop for emergency cleanup and profile_recover for retained native recovery. Neither operation replays a graph."],"retry":{"identity":"Canonical operationId UUID plus authenticated client identity.","sameArguments":"Replay returns the retained result or pending status without executing again.","changedArguments":"An operation_conflict is returned; use a fresh identity for a changed request.","retention":"128 operations per client, 16 clients, retained for this running MCP server lifetime. Capacity fails closed; no dedupe entries are evicted to admit new writes.","restart":"The volatile operation ledger and prepared drafts are lost. Reconcile profile/device/settings state before issuing a new operation identity."},"drafts":{"lifetimeMinutes":30,"maximum":16,"memoryBudgetBytes":MAX_DRAFT_BYTES},"localSettings":{"game-provider":{"selector":"gameId required","settings":"{connection, userFolder?, aircraftModule?}; game-specific native strict validation and current provider setup revision apply."},"desktop":{"settings":"{enabled:boolean, scope:{kind:'application'|'desktop-wide'}}; native captured application identities cannot be invented."},"selection-mode":{"settings":"{mode:'manual'|'automatic'}"},"game-rule":{"selector":"gameId required","settings":"{profileId,priority,enabled} or {remove:true}"},"game-preference":{"selector":"gameId required","settings":"{profileId} or {remove:true}"}},"bounds":{"requestBytes":MAX_REQUEST,"replyBytes":MAX_REPLY,"pageLimit":64},"security":"Loopback authenticated clients only. Reads never acquire outputs. Writes require server enablement and shared native ownership/admission. No arbitrary command, shell, filesystem, firmware update, game launch, publication upload or standalone device action invocation is exposed."})
}
pub fn profile_schema() -> Value {
    let examples = schema::examples();
    json!({"schemaVersion":osg_profiles::PROFILE_SCHEMA_VERSION,"maximumDocumentBytes":osg_profiles::MAX_PROFILE_BYTES,"contract":"ProfileDocument uses camelCase fields and deny_unknown_fields recursively. jsonSchema defines complete native wire shapes, including tagged processing operations, signal contracts, desktop actions and all typed edits. Native parse_draft and compiler remain authoritative for graph identities, contracts, quantity semantics and cross-field bounds. Portable documents contain no machine-local source pins, executable paths, process identities, claims or output owners.","jsonSchema":schema::document(),"emptyExample":osg_profiles::default_document(),"typedEdits":[
        {"kind":"set_name","name":"New profile name"},{"kind":"set_eligibility","eligibility":{"gameIds":["dcs"]}},
        {"kind":"upsert_resource","resource":examples["resource"]},{"kind":"remove_resource","id":"button-source"},
        {"kind":"upsert_binding","binding":examples["binding"]},{"kind":"remove_binding","id":"button-to-not"},
        {"kind":"upsert_processing_node","node":examples["processingNode"]},{"kind":"remove_processing_node","id":"invert-button"},
        {"kind":"replace_virtual_outputs","outputs":[examples["virtualOutput"]]},{"kind":"replace_desktop_outputs","outputs":[examples["desktopOutput"]]},
        {"kind":"replace_visibility","visibility":[examples["visibility"]]},{"kind":"replace_overrides","overrides":[examples["override"]]},
        {"kind":"replace_companion_assignments","assignments":[examples["companionAssignment"]]},{"kind":"replace_vehicle_tabs","tabs":[examples["vehicleTab"]]},
        {"kind":"replace_editor","editor":osg_profiles::default_document().editor}
    ],"authoring":"Copy exact typed port contracts from resource_inspect. Examples demonstrate shapes; native capability IDs and hashes must come from discovery. Removing a node leaves bindings visible for explicit repair and compiler diagnostics."})
}
pub fn assignment_schema() -> Value {
    json!({"schemaVersion":1,"contract":"LocalAssignment is stored beside a portable profile. Its native parser rejects unknown fields and validates exact graph/port membership.","example":[{"resourceId":"logical-profile-resource","sourceId":"native-resource-uuid","ports":[{"profilePortId":"logical-port","observedPortId":"exact-native-port-id","expectedContractHash":"sha256:exact-native-contract-hash"}]}],"bounds":{"assignments":64,"totalPorts":1024},"guidance":"Read resource_inspect and use its resource.id, descriptor.id and contractHash. A retained offline descriptor is authoring metadata; Apply recaptures native availability and ownership."})
}

/// Small validator for our explicit schemas. Native serde/graph parsers remain
/// authoritative for nested domain documents and opaque native route tokens.
pub(super) fn validate(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    validate_at(schema, schema, value, path, 0)
}
fn validate_at(
    root: &Value,
    schema: &Value,
    value: &Value,
    path: &str,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err(format!("{path} exceeds its nested schema budget."));
    }
    if let Some(reference) = schema["$ref"].as_str() {
        let key = reference
            .strip_prefix("#/$defs/")
            .ok_or("Invalid internal schema reference.")?;
        let target = root["$defs"]
            .get(key)
            .ok_or("Missing internal schema definition.")?;
        return validate_at(root, target, value, path, depth + 1);
    }
    for keyword in ["oneOf", "anyOf"] {
        if let Some(variants) = schema[keyword].as_array() {
            let matches = variants
                .iter()
                .filter(|variant| validate_at(root, variant, value, path, depth + 1).is_ok())
                .count();
            if matches == 0 || keyword == "oneOf" && matches != 1 {
                return Err(format!("{path} does not match a supported typed schema variant. Inspect the advertised $defs and exact discriminator/required fields."));
            }
        }
    }
    if let Some(expected) = schema.get("const") {
        if expected != value {
            return Err(format!("{path} must equal {expected}."));
        }
    }
    let types = schema
        .get("type")
        .map(|ty| ty.as_array().cloned().unwrap_or_else(|| vec![ty.clone()]))
        .unwrap_or_default();
    if !types.is_empty()
        && !types.iter().any(|ty| match ty.as_str() {
            Some("object") => value.is_object(),
            Some("array") => value.is_array(),
            Some("string") => value.is_string(),
            Some("integer") => value.as_u64().is_some() || value.as_i64().is_some(),
            Some("number") => value.is_number(),
            Some("boolean") => value.is_boolean(),
            Some("null") => value.is_null(),
            _ => false,
        })
    {
        return Err(format!("{path} has an invalid type."));
    }
    if let Some(options) = schema["enum"].as_array() {
        if !options.contains(value) {
            return Err(format!("{path} must be one of the advertised values."));
        }
    }
    if let Some(text) = value.as_str() {
        let length = text.chars().count() as u64;
        if schema["minLength"].as_u64().is_some_and(|min| length < min)
            || schema["maxLength"].as_u64().is_some_and(|max| length > max)
            || schema.get("pattern").is_some() && text.chars().any(char::is_control)
        {
            return Err(format!("{path} must be bounded printable text."));
        }
        if schema["format"] == "game-id" && !osg_profiles::activation::valid_game_id(text) {
            return Err(format!("{path} must be a valid game identity."));
        }
    }
    if value.is_number() {
        let number = value.as_f64().unwrap_or(f64::NAN);
        if schema["minimum"].as_f64().is_some_and(|min| number < min)
            || schema["maximum"].as_f64().is_some_and(|max| number > max)
        {
            return Err(format!("{path} exceeds its numeric bounds."));
        }
    }
    if let Some(items) = value.as_array() {
        if schema["maxItems"]
            .as_u64()
            .is_some_and(|max| items.len() as u64 > max)
            || schema["minItems"]
                .as_u64()
                .is_some_and(|min| (items.len() as u64) < min)
        {
            return Err(format!("{path} exceeds its item budget."));
        }
        if schema["uniqueItems"] == true {
            for (index, item) in items.iter().enumerate() {
                if items[..index].contains(item) {
                    return Err(format!("{path} must not contain duplicate items."));
                }
            }
        }
        for (index, item) in items.iter().enumerate() {
            let prefix = schema["prefixItems"]
                .as_array()
                .and_then(|prefix| prefix.get(index));
            if let Some(item_schema) = prefix.or_else(|| schema.get("items")) {
                if item_schema == false {
                    return Err(format!("{path}[{index}] is not a declared tuple element."));
                }
                validate_at(
                    root,
                    item_schema,
                    item,
                    &format!("{path}[{index}]"),
                    depth + 1,
                )?;
            }
        }
    }
    if let Some(object) = value.as_object() {
        if schema["maxProperties"].as_u64().is_some_and(|max| object.len() as u64 > max)
            || schema["minProperties"].as_u64().is_some_and(|min| (object.len() as u64) < min)
        {
            return Err(format!("{path} exceeds its property budget."));
        }
        let properties = schema["properties"].as_object();
        if let Some(key_schema) = schema.get("propertyNames") {
            for key in object.keys() {
                validate_at(root, key_schema, &json!(key), &format!("{path} key"), depth + 1)?;
            }
        }
        if schema["additionalProperties"] == false {
            if let Some(key) = object
                .keys()
                .find(|key| properties.is_none_or(|props| !props.contains_key(*key)))
            {
                return Err(format!("Unknown {path}.{key} field."));
            }
        }
        if let Some(required) = schema["required"].as_array() {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    return Err(format!("{path}.{key} is required."));
                }
            }
        }
        for (key, value) in object {
            if let Some(schema) = properties.and_then(|props| props.get(key)) {
                validate_at(root, schema, value, &format!("{path}.{key}"), depth + 1)?;
            } else if schema["additionalProperties"].is_object() {
                validate_at(
                    root,
                    &schema["additionalProperties"],
                    value,
                    &format!("{path}.{key}"),
                    depth + 1,
                )?;
            }
        }
    }
    Ok(())
}
