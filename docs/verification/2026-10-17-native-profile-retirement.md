# Native unified profile cutover

Native Manager no longer constructs standalone mapping, DCS/integration or
per-game hardware-profile runtimes. Retired commands are rejected before
selection, learning or admission effects, including standalone visibility Apply
and launch. Games uses fresh bounded installation metadata and game-owned
provider setup. Corrupt installation metadata blocks game identity, launch and
setup operations while preserving the file; ordinary hardware-only Apply remains
available. No old configuration is imported or used as fallback.

Retained game/output/provision/visibility/companion ownership is cleanup-only.
Exact native owners, journal fingerprints, exclusive locks, current qualifiers
and compare-if-unchanged restoration remain required. Cleanup cannot create an
old run, resume a mapping/script or silently reapply hiding. Saved companion
publication references remain protected against pruning.

The final frozen Windows native suite passed **705 tests**, with **3 explicit
prerequisites ignored**, zero failures and unchanged native/shared source stamps.
The separately requested official checksum-pinned DCS archive smoke passed.
Production coverage from that same full run plus archive smoke:

| Scope | Production coverage |
| --- | --- |
| Games installation-only runtime | 84.83% |
| Retained game cleanup | 86.06% |
| Fresh installation store | 95.38% |
| Retained companion codec and Hub cleanup owner | 100% each |
| Path anchor / exact cleanup composition | 92.31% / 98.08% |
| Retained DCS modules, 3 gates | 94.58–100% |
| Setup/context/session modules, 13 gates | 76.19–100% |
| Existing USB/profile helper modules, 35 gates | 74.29–100% |

All thirteen changed/retained integration modules meet the unchanged71% gate.
The Windows run of the complete integration runner correctly fails for the
unchanged `acr_windows` live OS adapter at19.18%; it requires separate live-game
evidence. The runner and its threshold were not weakened. Repository CI runs this
integration gate on Linux, where the explicitly unsupported adapter has its own
production path. No fresh Linux run is claimed here.

After measurement, fourteen unregistered old configuration/runtime sources and
their obsolete positive tests were removed. They were absent from the compiled
module tree during the full run; removal is followed by a fresh native build and
focused admission/cleanup regressions. Evidence before removal remains in
`.research/p9-integration-coverage` and `.research/p9-derived-coverage`.

CI service/E2E scenarios now author unified graphs, use actual resource/port
tokens and test physical Setter capture/owned output/restoration against the
pinned production-C simulations. Pure Python/Node/bootstrap oracles pass locally;
the full native service scenarios run on isolated CI hosts. No second native
service was started beside Cockpit dev. See the
[production-C proof](../profile-acceptance-verification.md). Physical USB, live
game, driver, visibility and companion qualification tickets remain open.
