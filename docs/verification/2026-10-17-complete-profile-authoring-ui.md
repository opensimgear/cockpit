# Complete profile rig through the browser UI

The extended run also creates, renames, changes membership and deletes editor
groups, focuses a canvas frame, collapses port categories, and checks Undo/Redo
and Save/reopen. The exported browser-authored document and its local assignments
pass the actual native parser and assignment validator: nine resources and 17
bindings. All 102 portable profile unit tests, four complete-rig tests and five
editor-metadata tests pass; all 15 native profile coverage gates exceed 71%.
The additional native validation exercises storage and schema, without activating
the authored graph or qualifying its hardware routes.

This is frontend fixture evidence for #79, #87 and #89. Microsoft Edge ran the production UI from the existing frontend on port 1420. The fixture intercepts every IPC before navigation and rejects unexpected commands and HTTP gateway fallbacks. It starts no service, native session, driver, device or game. Its Apply reply and sample pages are authored fixture captures, not proof of native activation, reduction, delivery or device acknowledgements. Qualification flags are unchanged.

`tests/ui/complete-profile-authoring.mjs` supplies only advertised inventory and retained-panel port metadata. It does **not** inject a final document, seed graph edges or bypass the authoring controls. Starting with Add, the browser:

- Selects exact device, game and companion ports, including a real one-control panel's separate widget inputs.
- Captures a hide-while-active intent for third-party C.
- Creates two named virtual layouts: A+B and a companion switch feed one; multiple CAN modules plus the direct USB module feed the other. Integer axes retain their declared native ranges.
- Rejects a direct event-to-action connection, then authors an explicit event argument pack and exact DCS command route.
- Authors visible feet-to-metres conversion, hysteresis, a setter argument pack, a display gate and text formatting. Exact routes carry DCS altitude to companion warning/text widgets and a requested physical indicator action.
- Checks that the single direct USB resource exposes both usable USB and CAN connection attributes without drawing transport edges.
- Undoes and redoes a route, arranges/fits the canvas, saves, reopens, and compares all 17 exact endpoint routes and six operation parameter sets with the repository complete-rig scenario. The production document and connection validators also check the captured save request.
- Explicitly applies the mocked saved revision, changes a draft transform label, and checks the source/transformation/destination view still shows the original applied revision and label. Fixture captures show `10000 ft`, `3048 m`, the formatted text, then distinct stale and unavailable samples. Inputs remain labelled upstream/requested values.
- Refreshes an offline inventory observation and retains its saved block and all routes. A locally created copy retains the graph while all nine resources are unassigned; cancelling it changes neither Default, the saved source nor the immutable applied owner.

The test passes with no unexpected IPC and no browser errors. It writes the actual UI-authored portable document, separate local assignments, command log and screenshots under `.research/complete-profile-authoring-browser/`. Native compiler/reducer evidence remains the independent four-case [complete portable rig verification](2026-10-17-complete-profile-rig.md).

Run against an already running frontend, with a locally installed Playwright module and browser:

```powershell
$env:OSG_PROFILES_UI_URL='http://127.0.0.1:1420'
$env:OSG_PLAYWRIGHT_MODULE='<absolute Playwright module directory>'
$env:OSG_BROWSER_CHANNEL='msedge'
pnpm profile-authoring:ui
```

Result on 2026-10-17: **1 passed**. This browser test has its own explicit script; the existing per-module 71% coverage gates and portable native integration test are unchanged.
