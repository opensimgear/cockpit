# Automatic port discovery and connection side panel

Verified against the running native dev Cockpit using real HTML5 palette drops,
mouse connection drags and native gateway IPC. No mock backend was used.

The connected VKBsim Gladiator EVO OT L advertised 135 outputs over five port
pages. Dropping it displayed all 135 automatically, before any port declaration
or route was saved. Output-to-input and input-to-output drags opened the right
side panel with the correct source and destination blocks. Search isolated the
chosen provider output and virtual input; Cancel left the graph and history
unchanged.

Creating the route declared and pinned just its selected real Boolean output.
Save, one Undo, Redo, another Save and reopening retained the exact route and
full discovered catalogue. One Undo removed both the route and its new port pin.
This also verifies the history fix that ignores native JSON object-key ordering
while retaining meaningful array order.

The same live test selected an existing compatible retained companion panel in
the inspector. Its full inputs and outputs appeared automatically without
browsing or selecting individual provider ports. Dragging a discovered companion
control to the virtual controller created one exact port pin and route; Save,
one Undo, Redo and reopening preserved the selected publication and both routes.

Automated checks:

- `pnpm check`: no errors or warnings in Cockpit or browser companion.
- `pnpm profile-workspace:test`: profile workspace, routing and wiring handlers.
- `node --test src/lib/profile-discovery.test.mjs src/lib/profile-graph-history.test.mjs src/lib/profile-wiring-ui.test.mjs`: 29 passing tests, including captured companion panel discovery, stale snapshots and atomic connection edits.
- `pnpm ui:build`: production builds for browser companion and Cockpit.
- `pnpm profiles:wiring:native`: real dev Cockpit acceptance, including save/reopen and cleanup of the temporary inactive profile.

Local screenshots and native evidence are in `.research/profile-wiring-native/`.
