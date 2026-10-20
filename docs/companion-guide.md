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

## Import an example and try it locally

1. Download [Flight button box](../examples/companion/flight-button-box.json) or [Racing axes](../examples/companion/axis-racing.json). These are portable panel bundles, without credentials or game bindings.
2. In **Companions**, use **Import panel** in the Panels list and select the file.
3. Choose **Save panel** to retain it. Check any unsupported-content or companion-update diagnostic before continuing.
4. Choose **Preview locally**, try the controls, then **Stop preview**. No virtual controller or game input starts.
5. Change pages or stop and check that controls return to their declared fallback. Feedback remains unknown until an explicit profile route supplies it.

The [example notes](../examples/companion/README.md) describe control behavior, logical target shapes and suggested feedback sources. Importing a panel does not configure a simulator or acquire an output device.

## Edit and save a panel

Use **New panel**, **Flight template**, **Racing template**, an imported panel or a saved Panels entry. Add supported widgets from the palette and edit their labels, positions and behavior. Inspect the intended viewport, rotation and pages. Small-target warnings need a layout adjustment and real-device review; a phone-sized preview is not touch qualification.

Momentary buttons release with the pointer. Toggles retain requested state until toggled again or cancelled. Pulse/encoder controls emit bounded pulses. Spring sliders return to their configured fallback; latched controls retain their value until cancellation. Page/layout changes and lifecycle cancellation clear held inputs. Logical axes, buttons and hats describe portable controls, not an OS device or a vJoy slot. Check their range and fallback.

**Save panel** retains the design and creates a fixed published copy internally; there is no separate Publish button. Select **Assigned companions** to update those assignments when saving. A failed companion update leaves the local save intact and reports the failure. **Export panel** creates a portable bundle without credentials, host trust, local output slots or game settings. Static PNG/JPEG/WebP assets travel inside it; external image URLs and animated assets are unsupported.

Panel changes create a new publication hash. Input behavior, ranges, fallback and logical bindings also affect the capability fingerprint. A profile keeps its selected published panel; saving a newer panel does not rewrite that choice or its active graph. Active ownership may block an assignment change. Deactivate/recover the owner and explicitly reconcile the profile's instance and ports before saving a replacement panel choice.

### Multiple viewports on one screen

Set **Companion screen** to the display's resolution, or enter its pixel width and height. The whole canvas represents that screen. Each page starts with one full-screen viewport; existing panels keep their layout.

Select **Add viewport** to use free space or split the selected viewport. Select an area from the **Viewports** list or its canvas label. Drag the label to move it, drag its corner to resize it, or enter pixel bounds in **Properties**. Its widgets move and scale with the area. Viewports can touch edges; overlapping or out-of-screen edits show an error and preserve the previous layout.

Select a viewport before adding widgets. A widget must fit entirely inside one viewport; dragging or editing it across an edge or gap is rejected. Move or delete an area's widgets before removing the viewport. Undo and redo cover viewport edits. **Preview locally** hides the editing guides; **Save panel** retains all areas as one panel for the assigned companion screen.

## Pair and assign a companion

For a desktop browser, follow [browser pairing](browser-companion.md): configure **Settings → Companion listener**, verify and trust its public certificate, then use **Companions → Pair → Browser**. Open the one-use pairing link in the intended browser. It expires after 60 seconds; keep it private. Cockpit does not change system trust or firewall rules. Loopback works only on the same computer.

Choose an **Assigned panel** in the paired companion's settings and save, or select the companion under a panel's **Assigned companions** before **Save panel**. The host supplies distinct instance identities. A display name is not an identity or credential. The browser receives the assignment automatically and starts after its verified panel and fresh neutral input baseline are acknowledged. Focus/background loss or reconnect requires a new session and baseline; old gestures are not replayed.

The native mobile shell retains its separate explicit Connect/Start lifecycle, but authentication remains gated. The host's **Mobile** pairing choice does not establish mobile qualification. Do not bypass that gate or certificate checks. ESP32 pairing and firmware remain shared device setup, independent of profile selection. See the [release matrix](companion-release-matrix.md) for each client.

## Route controls through Profiles

Open **Profiles**, then **Add profile** or an existing profile. A new profile opens an empty editor; use **Add from inventory** to add the companion. Select its block and **Browse published panels**. Choose the paired resource and **Published panel**, then **Use panel in profile**. **Remove selected panel** clears that panel choice while retaining the block's ports and routes.

Use **Inspect exact panel ports** and **Add control output** to select declared controls. **Connect exact ports** routes each selected output to a specific virtual-controller input or supported action. Add virtual layouts and visible processing nodes explicitly. Input learning and exact local assignments do not infer whole-panel mappings. Copied/imported profiles require explicit instance and port reconciliation.

**Save profile** stores the graph and local pins together and shows a notification. Saving the active profile applies its changes while preserving its game context and selection mode; saving an inactive profile leaves the active profile alone. Use **Activate profile** when ready. If another profile is active, confirm **Switch profile** to deactivate it and activate the requested profile. Activation checks backend readiness, exact capabilities, ownership and neutral baselines. Open **Debug** for preflight, applied samples and path activity; requested destinations do not prove OS or game delivery. Use designer preview for local control testing and Profile debug for keyboard/mouse recording.

## Add feedback

Feedback widgets are visual outputs; changing presentation does not press a control. Configure a simulator's installation, catalogue/aircraft, connection and freshness in its **Games** details. DCS-BIOS settings belong in DCS details. The profile graph selects actual catalogue telemetry outputs and the companion's declared feedback inputs. It contains no second integration configuration.

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

**Deactivate** releases owned contributions and leaves **No profile active**. Use **Activate profile** again, or explicitly select Automatic to resume automatic selection. Switching profile, game-context loss and peer disconnect invalidate affected state and require fresh baselines. Restoration compares the exact still-owned assignment/state and preserves external changes. **Recover outputs** handles the current profile's retained journal without replaying its graph, commands, held input or hiding.

Recovery records from the previous mapping/game system are cleanup-only. Open **Profiles → Debug** for **Previous output setup recovery**. Recover only the exact recorded owner; uncertain or externally changed state can remain pending. **Keep current setup**, when offered, explicitly abandons restoration ownership. Cleanup cannot start an old profile or migrate its configuration. Do not delete a journal to hide a conflict.

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
| Static raster assets | 2 MiB each, 8 MiB total; at most 8192 pixels per side, 16 megapixels per image and 32 megapixels total |
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
| Panel controls disarm on focus/page changes | This is lifecycle cancellation. A visible unfocused browser reconnects neutral for feedback only; focus requires another fresh baseline before controls resume. Hidden pages disconnect. Native mobile requires its explicit Start. |
| Live output blocked | Open Profile debug and read native preflight reasons. Check Settings output enablement, required drivers, assignments and neutral baselines. |
| Feedback unknown/stale or wrong units | Check exact profile ports, captured panel, game-owned catalogue/channel, visible conversion and current applied owner/context. |
| Pending recovery | Retain the journal and inspect its exact owner/conflict; cleanup does not restart a graph. |

For a bug report, include app/host versions, OS, instance/publication identity, preflight/fault text and reproduction steps. Redact pairing payloads, credentials and private trust material. Link the applicable [qualification story](companion-release-matrix.md) without marking an unperformed physical check passed. See the [Profiles guide](profiles-guide.md) for complete graph authoring and lifecycle behavior.
