# Automatic HidHide access for Cockpit

Ordinary Windows capture startup now grants access to its exact current executable
when the supported pinned HidHide driver is installed. This needs no profile or
elevation. It runs before the capture thread starts, once per native service.
Inventory/visibility requests, background refresh, test fixtures and read-only
qualification probes do not configure access.

The current image is resolved through a Windows file handle to its NT device path.
Normal mode adds that entry; inverse mode removes it. The private write adapter
accepts only the exact application-membership delta and retains every unrelated
application, hidden-device rule and global flag. General visibility write gates
remain unchanged. Pending or invalid standalone/game recovery journals block a
required change. Unsupported/busy drivers and uncertain write/readback outcomes
are reported rather than claiming success.

`controller_list.selfAccess` and `visibility.selfAccess` report the startup outcome.
Controller details display **Startup HidHide access**. Global Hidden status remains
independent of Cockpit's exception. Failed access leaves uncaptured controllers
unknown, with Forget unavailable; affirmative capture or PnP observations retain
their independently proven state.

## Actual running dev Cockpit

On 2026-10-18, the rebuilt Windows desktop ran against the existing dev server.
The same native services were tested through the running Cockpit browser UI on
`http://127.0.0.1:5175`, without IPC interception.

An independent read-only inspector used the native driver adapter directly, never
HidHideCLI, to snapshot the full policy before startup, after desktop startup,
after development-bridge startup, after passive UI polling and after restart:

- Baseline: normal mode, hiding active, five application entries and three hidden
  rules. The desktop executable was already permitted.
- Rebuilt desktop startup preserved the complete policy unchanged.
- Native development bridge startup reported `configured`, `ready: true`,
  `changed: true`, adding only its own exact executable as the sixth application.
  All prior applications/order, three hidden rules and global flags were preserved.
- UI details displayed the identical native result; automatic polling issued no
  visibility/configuration actions and left the full policy unchanged.
- After restarting the desktop and bridge, the bridge reported `unchanged`,
  `ready: true`, `changed: false`. The complete policy remained identical.

Evidence stays in ignored local research directories. SHA-256:

| Evidence | Digest |
| --- | --- |
| `.research/hidhide-self-access-inspect/before.json` | `e0e8741cf866ffa6b2f03c09568cb645d98f17cc549956ac9bc598ae88c9f2e8` |
| `.research/hidhide-self-access-inspect/after-service.json` | `cddd2701c008921446fed8fa4984be26898f08939fd572cd5fa05df124897fbc` |
| `.research/hidhide-self-access-inspect/after-restart.json` | `cddd2701c008921446fed8fa4984be26898f08939fd572cd5fa05df124897fbc` |

## Automated checks and limits

The controller crate passes 97 tests. New fixtures cover normal/inverse mode,
inactive/active hiding, exact case-insensitive membership, repeat startup,
external-policy preservation, recovery blocking, malformed/oversized policy,
restricted setters and uncertain write/readback outcomes. The unchanged 71%
per-module coverage gate passes; the new self-access module reaches 100% (55/55).
Inverse mode here is fixture coverage; the live driver check used normal mode.

Frontend checks and build pass, as do visibility rendering tests and the inventory
coverage gates. `pnpm controllers:hidhide:native:ui` checks the actual native
startup result, controller details and timer-driven passive polling. Set
`OSG_HIDHIDE_NATIVE_UI_URL` for a nondefault endpoint and `OSG_PLAYWRIGHT_MODULE` /
`OSG_BROWSER_CHANNEL` for the installed browser runtime. This check requires
Windows, supported installed HidHide and a remembered controller. Its startup
may configure the bridge executable's own access; its UI requests are read-only.

These checks establish Cockpit access setup and preservation of this machine's
policy. They do not qualify game compatibility or change device-hiding/profile
qualification.
