# Discovered game libraries — 2026-10-26

The catalogue now consists of discovered titles and installation instances.
Integration descriptors recognize compatible titles without creating catalogue
entries. Steam, Epic, Battle.net, EA/Origin, Ubisoft, GOG and Xbox scanners run
only when the corresponding client executable is installed; local paths remain
available independently.

First startup scans automatically. Rescan proposes Add/Update/Remove changes in
library accordions; Add/Update default selected and Remove requires selection.
Apply commits against the captured scan and library revision. Existing selected
executables are retained, while new defaults use platform evidence and fuzzy
filename/name, initials, sequel and helper filtering. Arbitrary title identity
merges require an explicit match instead of a fuzzy name guess.

Discovery rows contain artwork or a placeholder, name/executable selector, and
the action checkbox. Artwork fills the compact content height and uses bounded
384-pixel previews encoded directly from the source at JPEG quality 85. The
finished review pauses game-status polling and remembers accordion choices.
MCP exposes the same discovery and metadata actions with bounded mutation
receipts so operation replay does not retain image-heavy catalogue snapshots.

## Checks

- `cargo test --locked --manifest-path src-tauri/Cargo.toml -- --test-threads=4`:
  1,271 library tests plus two service/integration tests passed; eight
  environment-dependent tests remained ignored. The subsequently added five
  passive vehicle-view regressions also passed in the instrumented test binary.
- Frontend suite: 1,165 tests passed. The final artwork layout passed its 28
  focused tests, Svelte checks and production build. Workspace `pnpm check`
  passed. The profiles crate's 159 tests passed.
- Frontend game coverage: 98.63% lines overall, discovery helper 100%.
- `game-library-coverage.py`: all 24 production modules passed the 71% gate;
  minimum 75%. Artwork/backup modules measured 86.35%, 89.17%, and 93.42%.
- Profile resource coverage: 94.66% and 83.72%. Context/session/vehicle binding
  coverage passed; the new vehicle view module measured 96.10%.
- Coverage-tool regression suite: 37 tests passed. Include fragments are
  measured with their owning production module instead of disappearing from
  coverage reports.

## Running cockpit evidence

`tests/ui/game-discovery-native.mjs` passed against the native WebView with no
IPC replacement: library progress, immutable scans, two DCS copies, generic
metadata/profile lookup, moved installation identity, unchecked removal,
explicit removal, accordion closure, real executable defaults and platform
artwork. Fixture executables contained harmless text and were never launched;
cleanup removed only fixture installations.

`tests/ui/game-discovery-mcp-native.mjs` passed using the real MCP SDK and local
server: asynchronous exact-scan pagination, explicit identity matching, Apply,
identical operation replay, rename, installation selection and removal. The
temporary credential and fixture records were removed.

Final native visual measurement found a 384-pixel source displayed at about
143 pixels wide. Artwork and details shared the same vertical center; the
checkbox differed by less than 0.01 pixel. The image frame was about 80 pixels
high with centered cover cropping. Screenshots, measurements, logs and scan
evidence are retained under `.research/`, including `discovery-image-final.png`,
`discovery-image-final.json`, `game-discovery-native/evidence.json`, and
`game-discovery-mcp-native/evidence.json`.

These checks cover Windows software behavior and passive integration contracts.
No simulator executable, physical output or game launch was exercised.
