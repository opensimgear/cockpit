# Use a companion panel

Cockpit designs and hosts the panel. The browser companion displays it over the host's opt-in HTTPS listener; the native Companion app targets Android and iOS. Controller drivers belong on the Cockpit host when its output backend needs them. Configure those drivers and output enablement in **Settings → Controller and desktop output**. The companion computer needs no vJoy or HidHide.

Start with local preview. The [release matrix](companion-release-matrix.md) records installation, network and game-output qualification. **The qualified release matrix is currently empty.** Development builds and automated tests do not establish a signed, physically qualified release. Native mobile authentication remains disabled pending device qualification.

## Choose the right app

| Where | What to run | Current boundary |
| --- | --- | --- |
| Cockpit host | Native Cockpit | Owns panels, pairing, provider setup and the active unified profile. |
| Desktop browser | Cockpit-hosted browser companion | See [browser setup](browser-companion.md) for HTTPS trust, pairing and installation. |
| Android/iPhone/iPad | Companion mobile shell/local reference preview | Authentication remains disabled pending native device qualification. |
| Browser preview | Cockpit designer or mobile shell's browser preview | Local design/preview only; the mobile shell's browser preview cannot pair. |

A browser client does not select or qualify the host output backend. Use the [native build instructions](../apps/companion/README.md#development-and-native-builds) for development builds; signed installation/update paths remain in the release matrix.

Companion messages use information, warning and error icons on tiles, in settings and during setup. Hover an icon or focus it with the keyboard to read the full message. Press Escape to dismiss it.

## Design a widget and try it locally

The current designer starts with **Toggle**. Import the editable
[Toggle source](../examples/companion/generic-toggle.source.json) through
**Companions → Widgets → Import**, or choose **New** for a blank source. Old fixed
widget libraries and panel bundles were retired; their files are test fixtures.

Open a widget to configure **Design**, **Data**, **Behavior**, **Motion** and
**Sound**. Layers are editable authoring data. **Compile** produces complete
pixel presentations for the chosen target dimensions and media preferences.
**Try** runs that compiled result with separate input and feedback stimuli. No
host controller or game input starts from Try. Editing invalidates the compiled
result; compile again before Try, export or save.

Toggle changes its requested value immediately. Its game confirmation and
freshness remain independent. Unknown, stale and disconnected values have their
own presentations. Only fresh simulator feedback can establish game confirmation.

The artifact inspector shows source and parameter hashes, compiler/rasterizer,
capability profile, sample counts and pixel error, resources and media fallbacks.
Inspect at native target size. Motion and audio are optional; reduced motion or
unavailable audio leave state and live numeric feedback available. Browser audio
requires a user gesture, and suppressed cues are dropped.

See the [generic designer guide](generic-widget-designer.md) for the source,
behavior, compilation and device contracts. The [example notes](../examples/companion/README.md)
walk through the single pilot. Importing source never configures a simulator or
acquires an output device.

## Edit and save a panel

In **Panels**, choose **New** or open an existing card. Set its name and page
size, add a source from the palette, then drag, resize or enter exact pixel bounds.
Pages can have separate dimensions. Each instance pins an immutable source
revision and exposes that source's parameters. **Edit design** creates a new
revision and updates the selected instance after save.

Choose browser companions under **Assignments**, then **Compile** and **Try**
the complete panel. Try inputs stay inside the local simulator. **Save** stores
the source/layout and publishes a fixed compiled revision, updating the selected
assignments. A failed remote update leaves the local save intact and reports its
failure. **Export** includes the pinned sources, layout and verified compiled
result; **Import** requires a fresh compile for the current target.

The browser receives finished raster assets and bounded behavior. It does not
render authoring layers or use system fonts. PNG/JPEG/WebP artwork is decoded and
compiled on the host. External image URLs and executable source are unsupported.
ESP32 deployment is paused and tracked separately in feature #161.

Panel changes create a new publication hash. Input behavior, ranges, neutral
values and logical bindings also affect the capability fingerprint. A profile
keeps its captured published revision. Reconcile its instance and ports explicitly
when changing that choice. Active ownership can block assignment changes.

## Pair and assign a companion

For a desktop browser, follow [browser pairing](browser-companion.md): configure **Settings → Companion listener**, verify and trust its public certificate, then use **Companions → Pair → Browser**. Open the one-use pairing link in the intended browser. It expires after 60 seconds; keep it private. Cockpit does not change system trust or firewall rules. Loopback works only on the same computer.

Choose an **Assigned panel** in the paired companion's settings and save, or select the companion under a panel's **Assignments** before **Save**. The host supplies distinct instance identities. A display name is not an identity or credential. The browser receives the assignment automatically and starts after its verified panel and fresh neutral input baseline are acknowledged. Losing focus neutralizes controls while retaining the authenticated connection and feedback. Returning focus establishes a fresh control session and neutral baseline on that same connection; old gestures are not replayed.

The native mobile shell retains its separate explicit Connect/Start lifecycle, but authentication remains gated. The host's **Mobile** pairing choice does not establish mobile qualification. Do not bypass that gate or certificate checks. ESP32 pairing is temporarily disabled; compiled ESP32 deployment is in Backlog. See the [release matrix](companion-release-matrix.md) for each client.

## Route controls through Profiles

Open **Profiles**, then **Add profile** or an existing profile. Expand **Companions** in the **Components** palette and drag a published panel onto the canvas. This captures that exact panel revision and places its companion block in one undoable edit. You can also add an unconfigured companion by device name and choose **Published panel**, then **Use panel in profile**, in its inspector. **Remove selected panel** clears that panel choice while retaining the block's ports and routes.

Drag a block's output box onto another block's input box. The nearby picker filters the discovered ports by name or ID and shows their types, ranges and units. A sole compatible port is selected automatically; selecting the final port creates the connection. An occupied input requires **Replace connection**. Escape or a click on empty canvas cancels the operation.

For processing, drag a **Transformation** onto the canvas and select its type and settings. For example, drop **Multiply**, set **Multiply by** to 10, connect the integration output to it, then connect its output to a companion input. The first input establishes the signal contract and derives the output range. Unfinished transformations can be saved, but block activation until their input is resolved. Editing an established transformation checks all downstream contracts.

Select a wire to change ports, reconnect either end, insert a transformation, or disconnect it. You can also drag a selected wire endpoint to another block, drop a compatible transformation from the palette onto a wire, or press Delete to disconnect. Completed operations each form one Undo step. **Advanced connections** retains the detailed bindings table below the canvas. Input learning and exact local assignments do not infer whole-panel mappings. Copied/imported profiles require explicit instance and port reconciliation.

**Save profile** stores the graph and local pins together and shows a notification. Saving the active profile applies its changes while preserving its game context and selection mode; saving an inactive profile leaves the active profile alone. Use **Activate profile** when ready. If another profile is active, confirm **Switch profile** to deactivate it and activate the requested profile. Activation checks backend readiness, exact capabilities, ownership and neutral baselines. Open **Debug** for preflight, applied samples and path activity; requested destinations do not prove OS or game delivery. Use designer preview for local control testing and Profile debug for keyboard/mouse recording.

## Add feedback

Feedback widgets are visual outputs; changing presentation does not press a control. Configure a simulator's installation, catalogue directory, connection and freshness in its **Games** details. DCS-BIOS follows the loaded aircraft automatically. Select its controls and telemetry in the corresponding profile aircraft tab; Base exposes CommonData shared flight telemetry. The profile graph selects actual catalogue telemetry outputs and the companion's declared feedback inputs. It contains no second integration configuration.

In the companion block, inspect the captured panel's **Feedback / widget inputs** and choose **Add feedback input** for each needed property. Connect exact ports through visible unit conversion, comparison/hysteresis, Gate or Format nodes as needed. A destination action requires a complete ordered invocation, using an explicit **Action pack** or supported **Set on change** node. Types, units and declared ranges must fit. Visibility, enabled and navigation properties are available only when the renderer advertises their supported semantics.

For altitude, choose the actual aircraft catalogue channel and its defined quantity/units, rather than assuming a generic AGL or MSL value. Convert units and configure the condition and unavailable presentation explicitly. Only the matching verified game context may supply live game data. Wrong/no game, stale telemetry, disconnected peers and outdated panel publications cannot provide a fresh false/zero value or retain a Game confirmed presentation.

| Displayed provenance | What it establishes |
| --- | --- |
| Requested | The panel or graph requested this control state. |
| Local | Local/synthetic state, without simulator confirmation. |
| Host accepted | The host accepted the source value. |
| Host submitted | A native live feeder acknowledged output; a preview or graph sample cannot establish this. |
| Game confirmed | A supported fresh native simulator source reported the value; a button press alone cannot establish it. |

Freshness is separate from provenance. Unknown, stale and disconnected remain distinct; historical values are not current confirmation. An input acknowledgement with `submitted: false` does not confirm virtual output or game behavior. No Star Citizen telemetry source is added by this feature.

**Clear visual state** and **Resync visuals** in the companion affect presentation only. Resync requests a fresh native snapshot without replaying input or actions. Changed source semantics or panel contracts require explicit profile reconciliation.

## Activate, deactivate and recover owned state

A profile captures exact companion instance/publication/capability references with its graph and feedback routes. There is no separate global feedback-route save or Games companion capture. Ordinary hardware routing can work without a game; game integrations remain dormant until their matching verified context. Opening an editor leaves the active graph alone. Saving changes to that active profile applies them automatically; saving another profile does not switch to it.

**Deactivate** releases owned contributions and leaves **No profile active**. Use **Activate profile** again, or explicitly select Automatic to resume automatic selection. Switching profile, game-context loss and peer disconnect invalidate affected state and require fresh baselines. Restoration compares the exact still-owned assignment/state and preserves external changes. **Recover** handles the current profile's retained journal without replaying its graph, commands, held input or hiding.

Recovery records from the previous mapping/game system are cleanup-only. Open **Debug → Runtime & recovery** for **Previous output setup recovery**. Recover only the exact recorded owner; uncertain or externally changed state can remain pending. **Keep current setup**, when offered, explicitly abandons restoration ownership. Cleanup cannot start an old profile or migrate its configuration. Do not delete a journal to hide a conflict.

Removing a companion in its settings revokes its saved pairing; reconnecting then requires pairing again. Disabling the listener stops remote sessions without deleting panels or profiles. Browser credentials live in that browser profile's dedicated HTTPS origin; clearing site data or changing origin requires pairing again. The mobile shell's forget/protected-storage controls remain subject to its native authentication gate.

Host restart retains paired authority, listener preferences and saved panel references, but does not resume an old input session or replay prior commands/held controls. The browser reconnects with a new baseline; the mobile shell retains explicit Start. Automatic selection can admit a fresh saved graph after its recovery/identity checks; pending recovery blocks new ownership. The 1.5-second host lease bounds liveness loss while the host clock runs; a final frame cannot be guaranteed after an OS kill. Whole-host/feeder death has separately qualified cleanup limits.

## Limits and diagnostics

| Resource | Implemented limit |
| --- | --- |
| Companion instances acquired by one profile | At most 8, subject to shared runtime budgets |
| One panel | 16 pages, 128 logical inputs, 64 feedback widgets |
| Unified graph | 64 resources, 256 processing nodes, 512 bindings and 1024 graph ports; native virtual output supports at most 8 targets |
| Panel document / import bundle | 1 MiB document / 13 MiB bundle |
| Portable profile | 512 KiB; local resource assignments are separate |
| Compiled widget assets | 2 MiB each, 8 MiB total; 2048 pixels per widget dimension, 4096 frames, 64 MiB active + candidate memory |
| Input | At most 64 changes/batch, 120 batches/second/client, 512 queued transitions/client |
| Feedback | Up to 30 updates/second/client, latest-state coalescing |
| Native mobile offline cache | 4 verified panels, 24 MiB total; no offline host input |

The p95 25 ms/p99 50 ms touch-to-successful-virtual-submit numbers are qualification targets, not measured guarantees. Source acceptance, rendering and network round trips are separate boundaries. See the release matrix for load, real-game and platform evidence.

| Symptom | Check / next action |
| --- | --- |
| Mobile UI says BROWSER PREVIEW | This is the mobile renderer preview. Desktop remote input uses the [browser companion](browser-companion.md). |
| Mobile pairing unavailable | Native authentication is gated pending device qualification; use local references, not a flag override. |
| Another PC cannot connect | Check the listener's LAN/advertised address, stable port, certificate trust and firewall reachability. Loopback serves only the same PC. |
| Fingerprint, certificate or hostname error | Compare the host fingerprint, endpoint and clocks. Do not bypass TLS validation. |
| Panel saved, companion update failed | The local panel is retained. Inspect the exact assignment/ownership diagnostic and reconcile explicitly. |
| Selected published panel missing | Restore that published panel or explicitly reconcile the profile to another compatible panel. |
| Panel controls disarm on focus/page changes | Controls return to neutral while the browser connection stays available for feedback. Focus requires a fresh control baseline. Hidden tabs keep their connection while the browser can run them; suspension can expire the input session, which is reopened without replay. Unloading the page disconnects. Native mobile requires its explicit Start. |
| Live output blocked | Open Profile debug and read native preflight reasons. Check Settings output enablement, required drivers, assignments and neutral baselines. |
| Feedback unknown/stale or wrong units | Check exact profile ports, captured panel, game-owned catalogue/channel, visible conversion and current applied owner/context. |
| Pending recovery | Retain the journal and inspect its exact owner/conflict; cleanup does not restart a graph. |

For a bug report, include app/host versions, OS, instance/publication identity, preflight/fault text and reproduction steps. Redact pairing payloads, credentials and private trust material. Link the applicable [qualification story](companion-release-matrix.md) without marking an unperformed physical check passed. See the [Profiles guide](profiles-guide.md) for complete graph authoring and lifecycle behavior.
