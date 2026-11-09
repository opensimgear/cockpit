# Profile mode and editing in the running Cockpit

The top bar displayed Manual from `profiles.runtime`, but the first toggle
failed with “Refresh current local settings before saving.” The runtime
subscription had replaced the status read that initialized the settings
guard. The toggle now loads the native revision when the observed mode,
settings or owner changes. The read must match the displayed mode and exact
owner before the control can write. Losing the subscription retires the
guard and fences any late read.

The isolated Windows dev Cockpit used native revision `c8a57cb0` and the
`88ee864e` frontend with the candidate files. The live fixture switched
Manual → Auto → Manual. It recorded three selection-mode settings reads,
one on the initial baseline and one after each mode commit, with no repeat
in the 1.2-second idle window.

The fixture then created and saved profile
`9357c9fc-5cbb-44e8-aa76-4409007f3e9e`, activated it from its saved tile,
reopened the editor, renamed it and saved the active changes. The applied
revision and generation advanced from 1 to 2 within the same session.
Cleanup stopped the profile and deleted the temporary document. The fixture
reported no page or cleanup errors. Physical input qualification remains
in [#199](https://github.com/opensimgear/cockpit/issues/199).

The 25 adjacent UI and guard tests passed, with 100% line and function
coverage and 98.57% branch coverage for the guard. Svelte diagnostics on the
isolated candidate reported zero errors and warnings. The broader profile
workflow first reproduced the toggle regression. Its retry stopped on a
profile-name locator that did not match the isolated frontend, so the
focused live fixture used the actual saved-tile controls.

| Local artifact | SHA-256 |
| --- | --- |
| Native dev Cockpit executable | `4933f3a9c9bea2da4106484d24248e39f644095676473091109b7076a7ad4050` |
| `profile-editor-mode-fixture/evidence.json` | `71470d052a48daff4bf8e5df7d6c3292d848087a36a0dab51e5dd2c504fe5bed` |
| Private `profile-editor-mode-fixture.mjs` | `b254542abfba9894eb898019481d10268240951641caca48e27bc97abac70a8f` |

## Browser-dev with the native gateway

The `88ee864e` frontend with the committed mode fix ran in Edge through the
browser-dev bridge to the Windows native gateway built from `ec7348da`.
The browser workflow switched Manual → Auto → Manual with three settings
reads and no repeats in the 1.2-second idle interval. The profile list and
editor then created, activated and edited profile
`c73c16b2-4f10-45cf-bd60-2acd4e379ef9`. Saving the active profile advanced
its revision and generation from 1 to 2 within the same session.

The browser recorded two settings reads during activation and two during
the active save as the settings and owner projections advanced. The mode
control became writable on the current owner after each operation. Stop
and deletion completed with zero page or cleanup errors. A new test uses
the default header wiring to verify that changing the observed owner
requests another settings revision and an unrelated runtime message does
not request one. All 26 focused tests passed.

| Local artifact | SHA-256 |
| --- | --- |
| Browser-dev gateway executable | `7cd535cca53574c3d6410027a6f808e497ab0246c18bb8732454aa573fc653ae` |
| `profile-editor-mode-browser/evidence.json` | `086e227dffa15ac6f26d57d0e05665971971523cef6a4442bcaf0aa0005ec127` |
| Private `profile-editor-mode-browser.mjs` | `8fa6adeb4d592913d3e130d081d260459669f063567248a396a28aad6b203e0c` |

## Profile mode shared by the Windows header and MCP

The isolated Windows dev Cockpit from `c8a57cb0` ran the `88ee864e`
frontend with the committed mode fix. An SDK writer negotiated protocol
`2025-11-25` and a read-scoped client opened a `2026-07-28` subscription
to `cockpit://observations/profiles.runtime` on the same loopback server.
The subscriber opened on a Manual snapshot at revision 2, generation 1.

The writer saved Automatic using the exact native settings revision and
null active owner. The subscriber received a change at revision 3 and the
Cockpit header became writable in Automatic. A portal command carrying the
old revision was rejected and the MCP commit remained in effect. Clicking
the real header returned the app to Manual. The existing MCP subscriber
received the matching change at revision 6 in the same generation. The
final settings revision matched the initial Manual settings and there was
no active profile. Cleanup removed both credentials and restored the
disabled listener with zero errors.

| Local artifact | SHA-256 |
| --- | --- |
| Windows dev executable | `4933f3a9c9bea2da4106484d24248e39f644095676473091109b7076a7ad4050` |
| `mcp-profile-mode-focused/1a3b8205-2850-4ca9-aeb6-3e6400471e1b/evidence.json` | `cc9fd46185aaf037bdfb60830cc7b6c60cb96215a03f6aa5ab2546d6989b476b` |
| Private `mcp-profile-mode-focused.mjs` | `03bb8a29885142c280441f544f46856cca044a282cbf5968d3fa45ab42241a23` |
