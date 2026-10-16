# Use a companion panel

Cockpit designs and hosts the panel. OpenSimGear Companion displays it on another computer and sends its controls to Cockpit. Install controller drivers on the Cockpit host only when its separately qualified output backend needs them; the companion computer needs no vJoy or HidHide.

Start with local preview and recording. The [release matrix](companion-release-matrix.md) is the source of truth for supported installation, network and game-output combinations. **The qualified release matrix is currently empty.** Development builds and automated tests do not establish a signed, physically qualified release. Android/iOS authentication and unqualified host output capabilities remain disabled.

## Choose the right app

| Where | What to run | Current boundary |
| --- | --- | --- |
| Cockpit host | Native Cockpit | Owns publications, listener, assignments, mappings, simulator connections and game profiles. |
| Windows companion PC | Native OpenSimGear Companion | Native pairing/client implementation; physical second-PC, installation and lifecycle qualification remains open. |
| Linux companion PC | Native OpenSimGear Companion | Requires an unlocked Secret Service for credentials; native desktop/LAN qualification remains open. |
| macOS companion Mac | Native OpenSimGear Companion | Uses Keychain; installation, permissions and lifecycle qualification remains open. |
| Android/iPhone/iPad | Configured Companion mobile shell/local reference preview | Authentication remains disabled pending native device qualification. |
| Browser preview | Cockpit designer or Companion browser preview | Local design/recording; the browser Companion cannot pair with a host. |

A Linux or macOS **client** does not enable a Linux or macOS **host output backend**. Host output readiness is a separate check. Use the [native build instructions](../apps/companion/README.md#development-and-native-builds) for development builds; signed installation/update paths are tracked in the release matrix.

## Import an example and try it locally

1. Download [Flight button box](../examples/companion/flight-button-box.json) or [Racing axes](../examples/companion/axis-racing.json). These are portable bundles, not credentials or game bindings.
2. In Cockpit, open **Companion panels** and choose **Import bundle**. Select the downloaded JSON file.
3. Choose **Save draft**. The saved panel appears in the local Library.
4. Choose **Preview locally**. Try the controls and watch their recorded transitions. **Stop preview** ends the preview. No virtual controller or game input starts.
5. Change pages or stop and verify the controls return to their declared fallback. Feedback stays unknown until an explicit visual source is configured.

The [example notes](../examples/companion/README.md) explain control behavior, target shapes and optional feedback bindings. The examples make no game-specific binding assumptions. Importing them does not configure a simulator or acquire an output device.

## Edit and publish a panel

Use **New panel**, **Flight template**, **Racing template**, an imported bundle or a saved Library entry. Add widgets from the palette; edit their labels, positions and behavior in Properties. Use the page selector, **Add page**, viewport sizes and **Rotate** to check the intended screen. Choose **Phone inset** when reserving a safe area. Small-target warnings require a layout adjustment and real-device review; a phone-sized browser preview is not a touch qualification.

Momentary buttons release when the pointer releases. Toggle buttons retain their requested state until toggled again or the session is stopped. Pulse/encoder controls emit bounded button pulses. Spring sliders return to their configured fallback; latched controls retain their value until cancellation or Stop. A selector emits one normalized axis value, not several separate buttons. Page/layout changes and lifecycle cancellation disarm the panel and clear retained inputs.

Each input must have a **Logical target** binding before publication. Logical target layouts describe portable axes, buttons and hats. A logical target is not a vJoy slot or an OS device. Inspect axis range and fallback carefully: a bipolar axis uses -1 to 1, a unipolar axis uses 0 to 1, and fallback need not always be zero.

Choose **Save draft**, then **Publish revision**. Publishing creates an immutable revision. Later edits affect the draft; they cannot change an already published revision or a captured game profile. Choose **Export bundle** to share a portable copy. The export excludes installation credentials, host trust, local device slots and game settings. Static PNG/JPEG/WebP assets travel inside the bundle; external image URLs and animated assets are unsupported.

A visual change creates a new publication hash. Changes to input behavior, ranges, fallback or logical bindings also change its capability fingerprint. Updating a connected instance requires an explicit new publication/assignment and mapping reconciliation; it does not silently alter a running route.

## Pair a desktop companion

Keep Cockpit and the companion on the intended local network. Use native applications for this flow.

1. In Cockpit, expand **Companion connections**. Under **Published panels**, choose the saved **Local revision**, then **Make revision available**. This validates the revision and places its document/assets in the native host's memory cache.
2. Select **Listen on IP**, **Advertised host IP** and **Port**, then **Enable listener**. Loopback (`127.0.0.1`) works only on the same computer. For another PC, choose a reachable host LAN address and matching advertised address. Port 54437 is the default. Enable **LAN discovery** only if wanted. A bind conflict is reported; Cockpit does not silently select a replacement port or create firewall rules.
3. Choose **Pair a companion**. Transfer the complete one-time payload directly from the host screen through a trusted channel. It contains the endpoint, host certificate fingerprint and a secret that expires after 60 seconds. Do not post it in an issue, chat log or public screenshot.
4. In Companion, enter **This device name**, paste the **Manual / decoded QR payload**, and choose **Request pairing**. The app currently accepts pasted/decoded QR text; it has no built-in camera scanner. **Find nearby hosts** supplies hints only and does not establish trust.
5. In Cockpit, check the requesting installation and choose **Approve installation**. If the offer expires, generate a new offer and repeat the request.
6. Under **Controller assignments**, choose the **Installation**, enter a stable **Controller instance ID** such as `flight-left` or `racing-main`, select its published revision and choose **Assign panel**. Two copies of the same panel need different instance IDs. A display name is not an identity or a credential.
7. In Companion, choose **Refresh assignments**, select the **Assigned controller**, then **Connect**. Wait for the verified publication and fresh fallback-baseline acknowledgement. Choose **Start panel** explicitly when ready.

The native client checks the offered host CA fingerprint, certificate validity and endpoint name before sending the pairing secret. Compare trust information with the host screen. Do not bypass certificate errors or install a system-wide trust exception. Connection and **Start panel** are separate operations; pairing and assignment start no mapping or game output.

For same-PC testing, switching away from an active Companion window intentionally disarms it. Prepare the host's recording route before returning to Companion and choosing **Start panel**. A second computer avoids that particular focus switch, but still needs native LAN qualification.

## Record controls or inspect live readiness

In Cockpit's Mappings page, open **Companion controller mappings** and select the controller instance. Its immutable publication, full logical targets and axis fallbacks come from the published panel.

For a software recording, choose **Save for recording only**, then **Start selected recording**. Connect/start the panel explicitly and use its controls. The **Selected instance report log** shows the mapping engine's reports. This needs no virtual driver and does not prove that a game received input.

Live output additionally needs **Inspect native targets**, exact **Local output identity** and native axis-usage assignments, **Save exact output assignments**, and a completely passing **Native preflight**. Preflight checks qualification, explicit enablement, current publication, baseline, exact capabilities, resource ownership and recovery. **Start selected live output** stays unavailable while any required check fails. Do not change qualification flags to bypass this check.

Use **Stop selected instance** for this route. Stop cancels pending work and applies its declared fallback while other instances retain their own sessions. If cleanup remains pending, retain the recovery information and follow the displayed diagnostic. **Recover selected outputs** is available only when the native backend permits recovery. Do not delete journals or select another instance to conceal an unresolved cleanup failure.

After publishing a changed panel, stop the route and choose **Reconcile current publication** before saving/restarting its mappings. Reconnect and a new input baseline cannot revive a route bound to an obsolete revision.

## Add feedback

Feedback widgets are visual outputs. They never send a controller command.

To show simulator data, open **Simulator integrations**, select the provider and load its catalogue. Under **Logical telemetry for companion panels**, choose **Add logical telemetry source** and select a named channel. Use a stable **Source binding ID**. A source-only integration may omit physical device roles/outputs. Choose **Validate draft**, **Save validated profile**, then **Start integration** explicitly. Use the simulator's separately documented setup and supported channels; naming a channel does not create a telemetry feed.

Back in **Companion connections → Panel feedback**, select the controller instance. For each published output, choose its **Visual source**, then **Save visual routes**. Sources can be a published control's requested/accepted/submitted state or one of the integration's logical telemetry bindings. Types must match: indicators take booleans, text takes strings, and numeric/gauge widgets take numbers. A numeric conversion needs an explicit **Scale** or **Offset**, and the entire converted source range must fit the published widget range.

| Displayed provenance | What it establishes |
| --- | --- |
| Requested | The panel's requested control state. |
| Local | Local/synthetic state, including synthetic ACR telemetry. |
| Host accepted | The host accepted the source value. |
| Host submitted | A native live feeder acknowledged the corresponding output; recording alone cannot establish this. |
| Game confirmed | A supported fresh native simulator source reported the value. It does not follow merely from a button press. |

Freshness is separate from provenance. Unknown, stale and disconnected values remain visibly unavailable; a historical value is not a current confirmation. The input acknowledgement `source-accepted` with `submitted: false` does not confirm virtual output or game behavior. No Star Citizen telemetry source is added by this feature.

**Reset visual state** on the host, or **Clear visual state** in Companion, clears visuals until a new source change. **Resync visuals** requests a fresh native feedback snapshot. Neither action presses buttons, reconnects input, starts an integration or starts a mapping. When source semantics change, save the explicit route again rather than reusing an old source binding ID silently.

## Include assignments in a game profile

In Games, open a profile's **Captured companions** section. Optionally enter up to eight comma-separated **Controller instance IDs**, then choose **Capture current companion assignments** and **Save profile**. Leaving the field empty captures all current assignments only when there are at most eight; larger sets require an explicit selection.

The capture pins installation identities and immutable publication/capability hashes. It does not pair, connect, start a panel or start a mapping. Later designer edits do not modify this capture. Activation checks the captured revisions and feedback-source compatibility before changing settings. Active or recovery-owned companion routes must be stopped/cleaned up before the game session can take their assignments.

Choose **Restore setup** after the game session, or use recovery if restoration is pending. Cockpit restores only the exact assignments it owns. An external change is preserved and reported as a conflict. **Keep current setup** explicitly abandons restoration ownership and keeps the current setup; choose it only when that is the intended outcome. During an active game session, assignment/feedback edits are guarded. Emergency Stop, disconnect, revocation and visual reset/resync remain separate available operations.

## Stop, revoke and recover

- **Stop** in Companion disarms this panel and cancels held controls/timers. After page/orientation/focus/background loss or an error, Connect again if needed and choose Start explicitly. Old gestures are not replayed.
- **Disconnect input** in Cockpit releases the selected instance's writer. **Give selected installation control** changes that instance's authorized writer; the replacement still needs a new connection/baseline and Start.
- **Revoke access** invalidates one installation's credential and releases its sources. **Disable listener** stops remote sessions for the listener. Neither action deletes portable drafts.
- **Forget host** / **Forget saved pairing** removes this app's saved credential; it does not revoke the host's copy. Revoke obsolete installations in Cockpit as well.
- **Retry protected storage** retries an unavailable/locked native credential store. It restores valid authority disarmed. There is no plaintext fallback. Corrupt state is preserved until an explicit forget succeeds.
- **Open offline** displays one of up to four verified cached panels, disarmed with unavailable feedback. **Clear saved panels** clears public layouts only; it does not revoke credentials.

After a host restart, paired installation authority and offline source descriptors may remain, but the listener, active sessions and published asset cache do not resume. Choose **Make revision available** again for required revisions, explicitly enable the listener, then reconnect and Start. Game recovery may need those exact saved revisions before it can validate assignments.

The authenticated host lease expires after 1500 ms without liveness while the host/mapping clock can run. A final cancellation frame cannot be guaranteed after an app/OS kill. Whole-host or feeder death has backend-specific cleanup limits; the [release matrix](companion-release-matrix.md) keeps that qualification separate from normal Stop or a disconnected socket.

## Limits and diagnostics

| Resource | Implemented limit |
| --- | --- |
| Companion controller instances active together | At most 8, subject to shared mapping budgets |
| One panel | 16 pages, 128 logical inputs, 64 feedback widgets |
| Mapping workspace | 16 source roles, 8 total targets, 256 nodes across ordinary and companion routes |
| Portable document / import bundle | 1 MiB document / 13 MiB bundle |
| Static raster assets | 2 MiB each, 8 MiB total; at most 8192 pixels per side, 16 megapixels per image and 32 megapixels total |
| Input | At most 64 changes/batch, 120 batches/second/client, 512 queued transitions/client |
| Feedback | Up to 30 updates/second/client, latest-state coalescing |
| Companion offline cache | 4 verified panels, 24 MiB total |

The p95 25 ms/p99 50 ms touch-to-successful-virtual-submit numbers are qualification targets, not measured guarantees. Source acceptance, rendering and network round trips are different measurement boundaries. See the release matrix for load, real-game and platform evidence.

## Troubleshooting

| Symptom | Check / next action |
| --- | --- |
| Companion says BROWSER PREVIEW | Run the native Companion app. Browser preview has no remote pairing authority. |
| Pairing unavailable on Android/iOS | Native authentication is intentionally gated pending device qualification; use local references, not a flag override. |
| Another PC cannot connect | Check the LAN listen/advertised address, port, same-network reachability and OS firewall policy. Loopback only serves the same PC. Cockpit does not add a firewall exception. |
| Port already in use | Stop the competing listener or explicitly choose a free port and create a new trusted pairing payload. |
| Fingerprint, certificate or hostname error | Compare the full host fingerprint, advertised endpoint and system clocks. Stop on a mismatch; never disable TLS validation. |
| Address changed | Stop/disconnect, obtain a new trusted pairing payload, and re-pair explicitly. Revoke/forget the obsolete installation as appropriate; discovery cannot rewrite saved trust. |
| Waiting for approval / offer expired | Approve the correct installation in Cockpit, or generate a new 60-second offer and request again. |
| Instance already owned | Inspect its current installation. Disconnect its old input session or explicitly give the selected installation control. |
| Missing revision after host restart | Reopen the saved draft/publication and choose Make revision available for that exact hash. |
| Panel disarms after changing windows/pages | This is intentional lifecycle cancellation. Return to the panel, reconnect if needed and Start explicitly. |
| Live output remains blocked | Read each Native preflight reason. Recording can work while output is unqualified; preserve the disabled gate. |
| Feedback unknown/stale or wrong units | Check the exact published output, saved visual route, integration's validated source/channel/range, and explicit Start. Synthetic data stays Local. |
| Linux/macOS credential store unavailable | Unlock the native Secret Service/Keychain, then Retry protected storage. Qualification of a live native store remains distinct from injected-storage tests. |
| Pending output/game recovery | Keep the journal and inspect the conflict. Stop/recover only the affected ownership scope; Keep current setup is an explicit abandonment decision. |

For a bug report, include app/host versions, OS, instance ID, publication hash, relevant preflight/fault text and reproducible steps. Redact pairing payloads, credentials and private trust material. Link the applicable [qualification story](companion-release-matrix.md) instead of marking an unperformed physical check as passed.
