# Windows dev Cockpit parity audit, 28 November 2026

The source audit used committed `3f4dbe696efbd00129f0b4acef3d5fddce5af47f`. The live check used
the workspace's built debug executable with its running Vite server on port 1420, WebView2 on loopback
port 9227, and its normal development SQLite namespace. The live frontend included local work in
progress. Its profile inspector, desktop/processing/virtual editors, signal lanes and navigation had
changed action handlers. Those edits belong to a subsequent source review when they land.

## Observed outcomes

| Check | Result |
| --- | --- |
| Authenticated discovery | SDK 1.32.1 connected to Cockpit 0.1.0. Configure exposed 150 tools and read-only exposed 59. A separate fresh Configure session enumerated all pages. Its names exactly matched the audited source catalogue, including generated companion USB operations. |
| Credential administration | SDK-created reader, rotation and revocation reflected the same native revision. The retired credential lost access. The rendered settings showed credential metadata and hid the issued secrets. |
| Editing and activation | MCP edits reached the mounted editor. Stale dirty save failed. The native editor commit reached MCP. The suite created and saved a wired three-node profile, verified the native compiler result, reconciled a lost reply and checked exact activation ownership. |
| Resource and game discovery | Exact resource-port metadata agreed between native inspection and MCP. The SDK paged the processing and game vehicle catalogues. |
| Designer and companion publication | Designer revision stream agreed with the read snapshot. Read access excluded the publication write. The Configure client created and cancelled a pairing offer, uploaded with exact chunk offsets, committed and replayed the same operation. Native host state matched and stale prune was rejected. |
| Preferences | An SDK theme change appeared in the mounted Cockpit at the committed SQLite revision. Stale writes failed and the original preference was restored. |

The maintained entry point was `node --test tests/ui/mcp-native.mjs` with
`OSG_MCP_COMPANION_ACCEPTANCE=1` and `OSG_MCP_UI_PREFERENCES_ACCEPTANCE=1`. The first run reached the
native disabled-listener guard. With the companion listener enabled on loopback and discovery off, an
intermediate run timed out. The full rerun passed in 37 seconds and the listener returned to disabled.
The suite's Windows evidence recorded completion at 01:10:25 UTC. Only safe aggregate results are
transcribed here. The 30 frontend MCP settings and authorization-guard tests also passed.

The isolated candidate passed the 59 native MCP adapter tests. Its full native library test binary
then passed 2,497 tests on Windows with 19 explicitly ignored. A preceding sandboxed full-suite run
encountered loopback failures and exited with an invalid handle. The host run used the same binary.
The parity check passed for 70 workflows and 1,318 source action anchors. All ten checker regressions
passed, with 98.85% line, 81.67% branch and 89.47% function coverage.

This run verifies representative existing software behaviors. Installer elevation, signed mobile
authentication, physical board flashing, real simulator output and the planned assistant are outside
the executed scenarios. Platform and device qualifications retain their separate acceptance issues.
