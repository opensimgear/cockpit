# Optional Windows driver file observations

The system inventory worker registers one non-recursive native Windows watch
for the directory containing `vjoy.sys` and `HidHide.sys` before its initial
capture. The notification callback filters exact filenames and wakes the
existing worker. An overflow requests a fresh capture. A lost watcher can
rearm once until the next valid file event, with a bounded diagnostic if
notifications remain unavailable.

Each relevant event invalidates cached versions, including replacement with
the same size and modification time. The inspector also checks file metadata
after reading the version and reports unknown if the file changed during that
read. The existing explicit Refresh continues to inspect the installed files.

Driver metadata has a retained `system.inventory.drivers` snapshot. Settings
renders it directly and MCP exposes the same resource. General inventory
readers are invalidated when non-driver inventory fields change. The explicit
inventory command still includes current driver metadata.

## Running Cockpit

The initial desktop trace returned the same inventory revision twice after one
file insertion. The mounted Devices inventory and the Settings pane each read
that revision. With the scoped driver topic, the Windows desktop and Edge
browser-dev each showed a driver insertion, replacement and removal with zero
general inventory reads per driver change. The replacement preserved file size
and modification time. An unrelated filename produced zero inventory reads.
The second driver appeared after reopening Settings, with zero general reads
while the tab was closed and one opening inventory read.

A separate native SDK MCP smoke used a temporary read-only credential. It
listed `system.inventory.drivers`, read the absent, present and removed states,
and observed consecutive revisions 2, 3 and 4. The fixture restored the
listener setting and revoked its credential.

`tests/ui/optional-driver-observation.mjs` uses a private directory under
`.research`. The dev binaries replaced only the resolved driver directory in
an isolated source copy when `OSG_TEST_OPTIONAL_DRIVER_DIRECTORY` was set. The
test supplied versioned PE fixture files to the same native watcher and
version inspector. The fixture used file metadata only. Physical controller
acceptance remains in [#199](https://github.com/opensimgear/cockpit/issues/199).

The logs are `live-native-scoped-final.log`, `live-browser-scoped-final.log`
and `live-mcp.log` under `.research/reactive/optional-driver-observation/`.
The desktop executable SHA-256 was
`2A1520DE12A9A459620BED13B7E304710EC3DA0AA1029177FEAA3BE39D49ACF2`.
The browser-dev gateway SHA-256 was
`0DFC6A0BD97FA9A9A994C5AB2D5B6B72BD13CD90585B0F12D3E7887791D1C4B4`.

## Local checks

The final Windows native suite passed 2,407 tests with 18 default ignores.
The enabled native qualification test and both pinned DCS smokes passed
separately. All four measured production line gates passed with a minimum of
77.60%. LLVM function coverage ranged from 74.39% to 100%. The compiler did
not emit branch counters for these modules. The isolated frontend passed
all 1,699 tests, Svelte checking and the Vite production build.
