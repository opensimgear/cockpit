# Committed desktop output settings stream

The native `settings.desktop` observation starts with the persisted
enablement and scope. Successful native configuration publishes the next
snapshot to UI and MCP. Unchanged settings stay quiet. The snapshot excludes
application observations, held inputs and journal details.

The Devices settings view renders from this stream and clears its local
snapshot on loss. Refresh no longer reads desktop status. An explicit toggle
still captures the native settings revision and owner, and compares the
current settings with the displayed state before sending the guarded write.
Runtime focus and recovery details remain available through the explicit
`profile_desktop_status` read.

The running dev Cockpit verified that the MCP resource is discoverable, an
MCP settings save changes the Devices switch, and an explicit portal click
restores the initial value through the same native settings owner. The
fixture required an idle profile and restored the MCP listener configuration.

## Verification

- The native suite passed with 2,354 tests and 18 existing ignores. The new
  observation module covered all 20 production lines.
- All 13 focused frontend tests passed. Svelte diagnostics reported zero
  errors or warnings after linking the installed widget compiler dependencies.
  The broader check wrapper hit a pnpm workspace symlink permission error.
- The Windows dev build and the real MCP-to-portal round trip passed.
  Dev executable SHA-256 was
  `5830e7c3b21a20338968122d8c338d73af445e72b447b0b1d4bfb6581fd96f88`.
