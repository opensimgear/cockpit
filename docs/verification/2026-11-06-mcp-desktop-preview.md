# MCP desktop recording

The Debug panel's recording workflow is available through the read-only
`profile_desktop_preview` tool. Pass the compilation ID from
`profile_prepare_changes`, inspect its sources with `compilation_inspect`,
and supply bounded synthetic scalar frames and virtual clock advances.
The same native recorder serves the portal and MCP. It returns simulated
operations and acquires no output ownership or application scope.

The rebuilt Windows dev Cockpit advertised the tool to a temporary read
credential. The SDK and native portal command returned identical recording
results for an unsaved key-hold graph and the same compilation. Both results
included synthetic press and release with `nativeSubmission: false`.
An unknown compilation was rejected. Profile ownership remained empty.

The fixture also passed the existing desktop readiness and settings
roundtrip. The observation-only MCP suite and AI settings observation
fixture passed against the same running Cockpit. This qualifies recording
and the exercised status/settings workflows. Broader manual/MCP
cancellation and recovery parity remains under #197.

All 2,359 native tests passed with 18 existing ignores. The new catalogue
schema reached 21 of 21 production lines, and all 27 frontend MCP tests
passed.

## Input learning cancellation and idle recovery

Two final runs of `tests/ui/mcp-native.mjs` with
`OSG_MCP_OBSERVATION_ONLY=1` and `OSG_MCP_CONTROL_PARITY=1` passed against
the running Cockpit with an attached HID controller. A portal-native start
was cancelled through MCP, and an MCP start was cancelled through the
portal-native command. The terminal snapshots agreed, candidates were
withdrawn, and replay of the MCP cancellation returned the same result.
Cancelling the retired capture produced the same error through both paths
and left the newer capture listening.

The two paths also agreed on successful, non-cancellable idle recovery and
on rejection of an unknown activation ID. Neither run held an output owner
or had retained output receipts to recover. Pending activation cancellation
and recovery of retained hardware ownership still need live parity evidence.
