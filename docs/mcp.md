# AI configuration through MCP

Cockpit provides a local Model Context Protocol (MCP) server in the desktop
application. An assistant can inspect the same resources and profiles shown in
the UI, prepare changes, save a draft, and explicitly apply a saved revision.
The server shares the desktop's native manager and its device ownership,
configuration, and recovery rules.

## Connect a client

1. Run the Cockpit desktop app and open **Settings → AI integration**.
2. Choose a port if necessary, then turn on **Enable MCP server**. The default
   endpoint is `http://127.0.0.1:54438/mcp`. **Listening** confirms the native
   listener is running; an enabled preference alone does not confirm a connection.
3. Enter a client name and choose **Read only** or **Configure**, then select
   **Create token**. Create a different credential for each assistant.
4. Copy the token or select **Copy client configuration** while the new token is
   visible. Copying either hides the token. **Hide token**, leaving this settings
   section, or closing the app also removes it from the UI.
5. Add the connection to a client that supports authenticated MCP Streamable HTTP.
   Use the endpoint and an `Authorization: Bearer …` HTTP header. Start the
   connection and discover its tools and resources.

The copied JSON uses this common client configuration shape. Client-specific
settings may use different names for the server entry or header fields.

```json
{
  "mcpServers": {
    "opensimgear": {
      "type": "http",
      "url": "http://127.0.0.1:54438/mcp",
      "headers": {
        "Authorization": "Bearer YOUR_COCKPIT_TOKEN"
      }
    }
  }
}
```

The ordinary **Copy configuration** button copies a token placeholder. When a
new token is still visible, copying configuration includes that token and hides
it. If clipboard access fails, select and copy the visible text manually before
leaving the section. Cockpit cannot show an existing token again.

The listener accepts connections from this computer only. Keep Cockpit running
while using the client. LAN/remote hosting and a general REST API are outside
this implementation. The browser development gateway bridge is a different
development transport and does not provide these MCP settings. Running a second
gateway-service process against the same hardware is unnecessary.

## Access and credentials

**Read only** exposes inspection and draft preparation without configuration or
runtime changes. **Configure** includes those tools and permits explicit profile,
device, and supported local-settings changes. The native listener checks access
when requests arrive and again before dispatch; a client cannot obtain write
access by calling a tool hidden from its read-only catalogue.

**Replace token** creates a new credential with the same name and access, then
revokes the original. Update the client with the replacement token. If creation
fails, the original stays valid. If revocation cannot be confirmed, the UI keeps
the replacement token visible and tells you to refresh and explicitly revoke the
original. **Revoke** removes that credential's access. Credentials list their ID
so identically named replacements can be distinguished.

Only credential metadata and token hashes are saved in the machine-local
`mcp-v1.json` settings file. Plain tokens are returned once at creation and are
not stored in browser local/session storage or portable profile exports. The
native store supports up to 32 credentials; remove an unused one before creating
or replacing a token when that limit is reached. A damaged or unsafe settings
file is reported and preserved; the UI disables settings writes until it is
repaired.

Turning off the server closes its listener. Neither losing an MCP connection nor
turning off the listener is an instruction to deactivate an already applied
profile. Use the UI's **Deactivate** control or the explicit `profile_stop` tool
when that is intended. Requests already committed may have taken effect even if
the client never received their reply.

Revocation, disabling the listener, or replacing its port also fences pending
MCP activations before native takeover. Re-enabling access does not revive an old
preparation. Already committed runs remain active until explicitly stopped.

## Inspect, prepare, save, apply

The client should discover `tools/list` and `resources/list` for the exact
implemented schemas. The following context resources return bounded JSON:

| Resource | Purpose |
| --- | --- |
| `cockpit://guide/configuration` | Sequencing, reconciliation, limits, local-setting scopes |
| `cockpit://schemas/profile` | Complete structural JSON Schema, native examples and typed edits |
| `cockpit://schemas/local-assignment` | Machine-local source and port assignments |
| `cockpit://schemas/task-results` | Structured success/error result contract |
| `cockpit://status` | Current saved, selected, and applied profile state |

The [MCP contract and acceptance matrix](mcp-contracts.md) maps those schemas and
task families to native commands, invariants, errors, and verification paths.

Use this sequence for profile authoring:

1. Read `profile_status`, `profiles_list`, and `profile_get` for the saved
   document, its current revision, local assignments, and exact applied owner.
2. Discover `resources_list` and inspect relevant ports with `resource_inspect`
   or `resource_search_ports`. Copy their actual contracts and identities;
   display names, USB paths, and cached descriptors do not grant writer authority.
   `processing_operations_list` provides supported transformation kinds and
   their exact parameter schemas; filter by `search` or `category`, and page with
   `offset`/`limit`. `games_list` returns paged game summaries without nested
   aircraft/car arrays. Use `game_vehicles_list` with one `gameId`, optional
   `search`, and pagination to inspect that game's retained vehicle catalogue and
   current context. Catalogue/setup stamps identify the captured metadata; a
   listed vehicle does not prove live readiness or select it. Companion
   inspection provides retained panel authoring context.
   MCP profile reads preserve saved companion references, even when a newer
   panel publication exists. The UI's follow-latest refresh remains separate.
   Registering an installed aircraft authoring resource with
   `game_aircraft_resource` is an explicit **Configure** operation and requires
   an `operationId`; it is not a passive read.
3. Call `profile_prepare_changes` with an explicit `expectedRevision` and a
   complete draft or supported typed edits. New drafts use `expectedRevision:
   null`. Keep machine-local assignments separate from the portable graph.
   Preparation returns a client-owned `draftId`, a readable deterministic diff,
   and the native compiler report without saving or activating anything.
4. Inspect diagnostics with `compilation_inspect`. Fix invalid wiring and review
   resource readiness. An offline device and an invalid graph are different
   conditions; the native report remains authoritative.
5. Call `profile_save_prepared` with the exact `draftId` and a fresh
   `operationId`. Cockpit atomically stores the document and assignments and
   returns the resulting saved revision. A concurrent UI/AI edit rejects the
   stale base revision instead of overwriting it. An MCP save leaves the running
   revision unchanged, including when saving the currently active profile.
   If companion panel metadata changed and native normalization would replace
   the reviewed references, Save returns `resource_changed` without saving.
   Inspect the current panel metadata and prepare another explicit draft.
6. When activation is intended, refresh `profile_status` and call `profile_apply`
   with the saved ID/revision, the exact observed `expectedActiveOwner` or
   explicit `null`, the relevant `gameId` where needed, and a fresh
   `operationId`. The same operation ID identifies native activation progress.
   Cached previews do not authorize activation; native admission rechecks the
   current resources and ownership.
7. Inspect `activation_status`, `applied_ports_inspect`, and
   `applied_topology_inspect` as needed. `activation_cancel` cancels a specific
   pending activation; `profile_stop` performs explicit cleanup, and
   `profile_recover` retries existing cleanup/recovery work.

Prepared drafts live in memory for up to 30 minutes and belong to their
authenticated client. The adapter retains at most 16 drafts within an 8 MiB
budget. Restarting the desktop loses these drafts. Saved profiles survive
restart. Ordinary UI editor Save retains its existing behavior and may reapply
its own active profile; the MCP draft-only save is deliberately separate.

## Supported task families

| Work | Tools |
| --- | --- |
| Profiles | `profiles_list`, `profile_get`, `profile_status`, `profile_create`, `profile_duplicate`, `profile_rename`, `profile_delete`, `profile_import`, `profile_export` |
| Resource and processing discovery | `resources_list`, `resource_inspect`, `resource_search_ports`, `processing_operations_list`, `system_inventory` |
| Games and companions | `games_list`, `game_vehicles_list`, `game_telemetry_inspect`, `game_aircraft_resource`, `companions_list`, `companion_panels_list`, `companion_inspect` |
| Prepared changes | `profile_prepare_changes`, `compilation_inspect`, `profile_save_prepared` |
| Applied runtime | `profile_apply`, `activation_current`, `activation_status`, `activation_cancel`, `profile_stop`, `profile_recover`, `applied_ports_inspect`, `applied_topology_inspect` |
| Gateway/CAN setup | `gateways_list`, `gateway_inspect`, `gateway_session`, `gateway_config_read`, `gateway_config_change` |
| Direct USB setup | `usb_modules_list`, `usb_module_inspect`, `usb_module_session`, `usb_module_config_read`, `usb_module_config_change` |
| Device preparation | `device_config_prepare` |
| Machine-local settings | `local_settings_get`, `local_settings_save` |
| Mutation reconciliation | `operation_status` |

Device setup uses the existing native claim, boot, route, generation, validation,
and readback behavior. Inspect fresh native state before opening a session and
after a change. `device_config_prepare` reads the exact USB/gateway configuration,
validates a candidate with the native config encoder, and returns a deterministic
diff without claiming, applying, or saving. Its baseline is review context;
explicit setup still checks native ownership and routes, and firmware acceptance
is an Apply result. Apply and durable Save remain distinct outcomes; use the
advertised action enum to request either explicitly. A competing UI writer,
reconnect/reboot, ambiguous USB/CAN identity, or applied profile can reject setup.
Do not reconstruct a route token or silently move a draft to another transport.

Local settings support the `game-provider`, `desktop`, `selection-mode`,
`game-rule`, and `game-preference` scopes. Read the scope first, then save with
its returned `expectedSettingsRevision` and observed `expectedActiveOwner`.
Scope-specific native validation rejects unsupported fields. These local
preferences are separate from portable profile data. Changes to automatic mode
or a matching enabled rule can cause native automatic activation; review and
authorize that runtime effect before saving them.

Desktop output controls, profile mode and remembered game choices in the UI
also retain their displayed native settings revision and owner. A concurrent
MCP edit makes an older UI save report a conflict and require a refresh; the UI
does not fetch a newer revision at Save time to authorize an old form.

### v1 capability matrix

Every `local_settings_save` requires **Configure** access, an `operationId`, the
exact `expectedSettingsRevision` from a fresh read, and the observed
`expectedActiveOwner` or explicit `null`. The shared native admission checks
revision and owner together. A competing UI/client edit or external settings-file
change is rejected rather than overwritten. Read responses can include
`readOnly`, a nested `rule`, and other status metadata; construct the save's
`settings` from the accepted fields below instead of sending the entire read
response back.

| Local scope / selector | Accepted `settings` | Runtime effect and additional native guards |
| --- | --- | --- |
| `game-provider` + supported `gameId` | Required `connection`; DCS alone also accepts `userFolder` and `aircraftModule` | Persists the provider connection/catalogue preferences and setup revision without launching a game or directly applying a profile. Game-specific connection validation, existing storage checks, and provider/native game admission still apply. DCS folders/modules must pass native path/catalogue validation; process identities and executable paths are not accepted setting fields. |
| `desktop` · no `gameId` | `{enabled:boolean, scope:{kind:"application"\|"desktop-wide"}}` | Reconfigures the machine's desktop-output capability. Native configuration first deactivates the current applied run and pins Manual mode; saving never injects input or applies a profile. Requires the native app, current desktop preferences revision, cleanup success, and existing platform/output gates. This interface accepts no PID, executable path, application identity, or application-capture command. |
| `selection-mode` · no `gameId` | `{mode:"manual"\|"automatic"}` | Persists the selection policy. Explicit Automatic can clear the native automatic-control stop pin and allow a matching verified context to activate a profile; Manual pins the current selection. Snapshot/owner guards, read-only settings, conflict handling, and native staged activation gates remain authoritative. It does not launch a game. |
| `game-rule` + supported `gameId` | `{profileId, priority, enabled}` or `{remove:true}`; priority is an integer from −32768 to 32767 | Saves/removes that game's automatic activation rule. Enabled rules require an eligible saved profile and a current native game activation identity; disabled-rule edits retain only allowed existing identity evidence. In Automatic mode the changed rule may affect activation. Shared automatic-settings revision, owner, eligibility, verified game identity, and normal takeover/recovery gates apply. |
| `game-preference` + supported `gameId` | `{profileId}` or `{remove:true}` | Remembers/removes the preferred saved profile for that game context. A saved choice must match the profile's saved game context. This updates later native profile choice without issuing a game launch or direct Apply. The shared automatic-settings snapshot, owner, profile-context validation, and storage guards apply. |

Supported game IDs are `dcs`, `x-plane-11`, `x-plane-12`,
`assetto-corsa-rally`, `assetto-corsa`, `assetto-corsa-competizione`, and
`assetto-corsa-evo`. Provider `connection` objects have these native field sets:

| Provider | Accepted connection fields / validation |
| --- | --- |
| DCS | `cataloguePath`, `freshnessMs`, `commandHost`, `commandPort`, `listenAddress`, `listenPort`, `multicastGroup`, `multicastInterface`. The catalogue path is an absolute validated local preference; UDP addresses/ports use native DCS network validation. `userFolder` and optional `aircraftModule` are sibling provider settings, not connection fields. |
| X-Plane 11/12 | `source:"rref"`, matching numeric `version`, IPv4 loopback `host`, `port` 1–65535, `listenPort` 0–65535, `updateHz` 1–20, and `freshnessMs` 100–10000. Freshness must allow at least two requested updates. No stored runtime process identity is accepted. |
| Assetto family, including Rally | `source:"shared-memory"` and `freshnessMs` 100–10000. Runtime-build, process, setup-readiness, and existing provider qualification checks remain independent facts. |

| Other v1 capability | Supported interface | Runtime / qualification boundary |
| --- | --- | --- |
| Direct USB module setup | Read exact module/key/route, prepare a config diff, explicitly claim/apply/apply-and-save/save/discard/reset/release as advertised | Uses the native route token, verified identity, boot, connection/configuration generation, writer claim, and active-owner checks. No silent draft migration; reconnect/reboot requires fresh evidence. Applied and durably saved results remain separate. |
| Gateway/CAN module setup | Discover/connect an exact gateway, read its module, prepare a config diff, and use the advertised explicit setup actions | Uses the same native gateway/module session, CAN boot/route/writer/lease proof, device readback, USB preference, and cross-transport exclusion as the UI. No ambiguous write replay. Physical CAN/USB behavior is not qualified by an MCP preview. |
| Module configuration fields | Native `RuntimeConfig`: `name`, `expanderMask`, `buttons`, `axes`, `hats`, `encoders`, `selectors` | Native encoder, array/range limits, advertised hardware capabilities, firmware semantic acceptance, and calibration/layout validation apply. The typed tool schema is the exact contract; MCP does not expand an adapter's device capabilities. |
| Companion descriptors and profile assignment | Inspect retained companion identity/ports and verified panel publications, then reference exact publication hashes and local assignments in a prepared profile | Assignment persistence uses atomic draft save; runtime publication/ownership starts only through explicit profile activation. Existing pairing/trust, screen compatibility, retained publication, and resource-generation checks apply. Credentials and machine paths stay out of portable profiles. |
| Deliberate v1 exclusions | Remote/LAN MCP hosting, general REST API, high-frequency telemetry streaming, arbitrary command/shell/filesystem access, firmware/driver provisioning, game launching, standalone device actions, publication upload, and full visual companion authoring | Use the existing native UI/workflows or separately planned features. MCP tools cannot bypass platform, game, output, or hardware enablement and recovery gates. |

| Verification scope | Current claim |
| --- | --- |
| Windows x64 native development desktop | Initial live software verification uses the running Cockpit and official JavaScript SDK 1.32.1 over negotiated `2025-11-25`. Native IPC/UI agreement, authentication, scopes, schemas, and workflow outcomes are recorded separately in dated verification evidence. |
| Windows x64 packaged application | Initial packaged startup/client checks passed, including the official SDK connection. Raw modern `2026-07-28` requests also exercised required typed metadata/headers, malformed content-type rejection (415), and oversized body rejection (413). This is software evidence, not installer-distribution or physical-output qualification. |
| Linux, macOS, and other desktop architectures | Source and repository CI/native checks remain relevant, with their existing platform gates. This Windows session makes no local native-client, packaged-app, or hardware acceptance claim for these targets; record their own CI and platform-specific results before expanding the declared verification scope. |
| Physical devices, game bindings, and output hardware | Qualification is explicitly deferred by the user for this feature. Software or simulator results do not establish electrical CAN, physical USB/HID enumeration, firmware/calibration behavior on hardware, simulator binding retention, or physical output behavior. |

## Conflicts, disconnects, and limits

Every mutating tool requires a canonical lowercase UUID `operationId`. Keep the
same ID and exact arguments when reconciling a request whose outcome is unknown.
`operation_status` returns retained pending/completed results for the same
authenticated client. Reusing the ID with different arguments is an
`operation_conflict`; a changed, reviewed action needs a fresh ID.

The in-memory ledger retains up to 128 operations per client and 16 clients,
without evicting earlier identities to admit a new mutation. Capacity errors
fail closed. The ledger lasts for the running server object's lifetime; a
desktop restart loses it. After restart or an `operation_unknown` result, inspect
the actual profile/device/settings state before deciding whether new work is
needed. Never treat a timeout, credential revocation, disconnect, or cancellation
as proof that an already committed write was rolled back.

Task results use `ok` plus `data`, or `ok: false` plus a structured error with
code, message, and reconciliation guidance. Saving, pending activation, running,
degraded/fault state, and durable device persistence must remain distinguishable.
Native Stop/cancel has a separate request budget so ordinary catalogue traffic
does not consume all available control capacity.

Requests are bounded to 2 MiB. General catalogue pages accept up to 64 items;
some native inventories have smaller limits. Follow returned pagination and
request smaller pages or a specific inspection section when a reply is too
large. The installed tool schemas and compiler diagnostics specify additional
document, catalogue, and native limits.

## Compatibility and troubleshooting

The server uses the pinned Rust SDK `rmcp = 3.6.0` with Streamable HTTP and
advertises protocol versions `2025-11-25` and `2026-07-28`. The settings panel
shows the advertised versions. It supports tool and resource discovery; clients
must use capabilities actually advertised by the server. Use a client with
Streamable HTTP and custom bearer-header support. No stdio proxy is bundled by
this implementation.

The development client dependency is pinned to
`@modelcontextprotocol/sdk = 1.32.1`. Verification records identify the client
and protocol version actually exercised; the dependency pin alone is not
connection evidence.

- **Desktop app required:** the browser UI cannot manage the desktop listener.
- **Unable to start / address in use:** disable the server, choose another port
  from 1024–65535, and enable it again. Update the client endpoint.
- **Unauthorized / revoked / no mutation access:** check the token, its access,
  server enablement, and whether it was replaced or revoked. Reconnect with a
  current credential.
- **Revision/owner/capability conflict:** inspect current native state and review
  the change again; do not substitute a newer version automatically.
- **Unknown or expired draft/preview:** prepare and compile the current draft
  again. Preparation itself does not save anything.
- **Uncertain native result:** reconcile the original operation ID and current
  native state before submitting further writes.
- **Read-only settings file:** preserve the reported file and repair its
  corruption, permissions, or unsafe path; the server does not silently reset it.

## Verification and readiness

Frontend contract and component checks can be run with:

```sh
node --test src/lib/mcp.test.mjs src/lib/mcp-ui.test.mjs
pnpm exec svelte-check --tsconfig ./tsconfig.json
```

These checks cover the settings protocol, credential replacement/partial
failure, one-time secrets, stale reads, local-only configuration, and component
compilation. They do not establish a live MCP client connection or qualify
physical output hardware. Native protocol/integration checks and a real client
must additionally exercise the running dev Cockpit and compare its UI state.
Record actual commands and observed results in `docs/verification/`; a workflow
described in this guide is not itself acceptance evidence.

For this feature, physical hardware validation is deferred at the user's
direction. Software implementation, native simulated acceptance, and client
connection checks must report their own evidence without claiming USB/HID,
electrical CAN, firmware, simulator bindings, or physical output qualification.

Implementation milestones and acceptance are tracked in
[feature #119](https://github.com/opensimgear/cockpit/issues/119).
