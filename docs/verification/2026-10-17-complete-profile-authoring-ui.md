# Complete profile rig through the browser UI

The earlier editor-metadata run creates, renames, changes membership and deletes editor
groups, focuses a canvas frame, collapses port categories, and checks Undo/Redo
and Save/reopen. The exported browser-authored document and its local assignments
pass the actual native parser and assignment validator: nine resources and 17
bindings. All 102 portable profile unit tests, four complete-rig tests and five
editor-metadata tests pass; all 15 native profile coverage gates exceed 71%.
The additional native validation exercises storage and schema, without activating
the authored graph or qualifying its hardware routes.

The policy extension selects **Disable while active** for CAN A through its
resource inspector, checks the descriptor/reconnect warning, Undo/Redo and
Save/reopen, and retains the exact false intent in portable export. The policy
helper/component suite passes 45 cases and graph coverage exceeds all 71%
gates. Native publication admission and physical effects remain separate from
this intercepted frontend fixture.

The raw-quantity extension uses the CommonData `ALT_MSL_FT` catalogue contract:
an integer from 0 to 65535, no declared unit, 1500 ms freshness and unknown
fallback. The mocked provider advertises the exact observed port ID
`channel:common:CommonData:ALT_MSL_FT:0`. That identity and its contract hash are
captured only in local assignments. The saved portable graph now has 18 exact
routes and seven visible processing nodes. A separate **Interpret MSL altitude**
node explicitly declares **Reported MSL altitude**, in `ft`, before conversion.
Its input and output retain the same integer type, range and value. This is the
author's interpretation of the documented exported quantity, not an inferred
unit, real DCS observation or provider acknowledgement.

This is frontend fixture evidence for #79, #87 and #89. Microsoft Edge ran the production UI from the existing frontend on port 1420. The fixture intercepts every IPC before navigation and rejects unexpected commands and HTTP gateway fallbacks. It starts no service, native session, driver, device or game. Its Apply reply and sample pages are authored fixture captures, not proof of native activation, reduction, delivery or device acknowledgements. Qualification flags are unchanged.

`tests/ui/complete-profile-authoring.mjs` supplies only advertised inventory and retained-panel port metadata. It does **not** inject a final document, seed graph edges or bypass the authoring controls. Starting with Add, the browser:

- Selects exact device, game and companion ports, including a real one-control panel's separate widget inputs.
- Captures a hide-while-active intent for third-party C.
- Creates two named virtual layouts: A+B and a companion switch feed one; multiple CAN modules plus the direct USB module feed the other. Integer axes retain their declared native ranges.
- Rejects a direct event-to-action connection, then authors an explicit event argument pack and exact DCS command route.
- Rejects Unit Convert on the raw unitless altitude. It authors the explicit MSL/feet declaration through its quantity and unit fields, checks Undo/Redo, then adds feet-to-metres conversion, hysteresis, a setter argument pack, a display gate and text formatting. Exact routes carry the declared altitude to companion warning/text widgets and a requested physical indicator action.
- Checks that the single direct USB resource exposes both usable USB and CAN connection attributes without drawing transport edges.
- Undoes and redoes a route, arranges/fits the canvas, saves, reopens, and compares all 18 exact endpoint routes and seven operation parameter sets with the repository complete-rig scenario. The production document and connection validators also check the captured save request. The raw source contract, exact local observed ID/hash and unchanged integer schema across the declaration are asserted separately.
- Explicitly applies the mocked saved revision, changes a draft transform label, and checks the source/transformation/destination view still shows the original applied revision and label. Authored fixture captures show raw `10000`, declared `10000 ft`, converted `3048 m`, the formatted text, then distinct stale and unavailable samples, including both sides of the declaration. Inputs remain labelled upstream/requested values.
- Refreshes an offline inventory observation and retains its saved block and all routes. A locally created copy retains the graph while all nine resources are unassigned; cancelling it changes neither Default, the saved source nor the immutable applied owner.

The test passes with no unexpected IPC and no browser errors. It writes the actual UI-authored portable document, separate local assignments, command log and screenshots under `.research/complete-profile-authoring-browser-raw-quantity/`, retaining the earlier browser artifacts separately. Native compiler/reducer evidence remains the independent four-case [complete portable rig verification](2026-10-17-complete-profile-rig.md).

Run against an already running frontend, with a locally installed Playwright module and browser:

```powershell
$env:OSG_PROFILES_UI_URL='http://127.0.0.1:1420'
$env:OSG_PLAYWRIGHT_MODULE='<absolute Playwright module directory>'
$env:OSG_BROWSER_CHANNEL='msedge'
pnpm profile-authoring:ui
```

Result on 2026-10-17: **1 passed**. This browser test has its own explicit script; the existing per-module 71% coverage gates and portable native integration test are unchanged.
