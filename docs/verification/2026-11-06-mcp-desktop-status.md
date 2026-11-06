# MCP desktop output status parity

The MCP catalogue exposes a read-only `profile_desktop_status` tool. It calls
the same native keyboard and mouse output status command as the portal. Its
bounded result includes machine enablement, platform qualification, current
readiness and recovery diagnostics. When present, the runtime summary includes
waiting controls, pending dispatch and scope notification health.

The tool returns a summary rather than the complete native diagnostic payload.
Application and helper process identities, backend journal details and dispatch
history stay in native diagnostics. The tool neither starts outputs nor edits
settings. It is an explicit snapshot read. Live desktop status observation
and broader manual/MCP parity remain open.

## Verification

- Native suite passed with 2,353 tests and 18 existing ignores. The MCP tools
  module covered 605 of 772 production lines (78.37%).
- All 27 MCP frontend tests passed. The audit verified 247 timing dispositions.
- The Windows dev build passed. The running Cockpit matched the portal
  status with a read-only MCP credential, kept the owner unchanged, and
  passed the AI settings observation regression. The test restored the
  listener setting and revoked its temporary credential.
- Dev executable SHA-256 was
  `395b3292c542fdb5872d6964da9f79f8c771048bea854055ed4625245093f5d7`.
