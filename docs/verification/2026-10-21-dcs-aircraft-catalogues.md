# DCS aircraft catalogues and automatic modules

The running native dev Cockpit was rebuilt and exercised on Windows with the
installed DCS-BIOS 0.11.7 catalogues. Gateway IPC, native process verification,
UDP transport, profile storage, compilation and runtime inspection were real.
The test owns an inert Windows executable named DCS.exe and authors DCS-BIOS
export packets; it does not run or qualify a simulator flight or physical output.

`tests/ui/dcs-aircraft-catalogues-native.mjs` creates the profile through the
actual editor, with Base, A-10C_2 and F-16C_50 tabs. Aircraft integrations pin
different native catalogue resources. A-10C_2 resolves to the A-10C module; its
AAP_CDUPWR command is absent from the F-16 catalogue. Base uses read-only
CommonData. Browsing either aircraft leaves provider preferences and live
module authority unchanged.

The applied graph receives altitude 1000 through Base and a native aircraft
integer value 1 through each matching aircraft tab. Switching to F-16 replaces
the A-10C nodes and owner generation, and rejects inspection using the old owner.
Both aircraft retain their owner through recurring catalogue refreshes. Stale
exports select Base alone; fresh exports restore the alias-selected A-10C tab.
Stop prevents reactivation while passive aircraft discovery continues.
The visible aircraft canvas also rediscovers its complete controls and telemetry
after native route changes. A failed paged read retries on a changed inventory
snapshot; unchanged snapshots do not retry, and late reads cannot replace newer
catalogue results or edit the graph.

Live testing also covered the installed alias table's NONE sentinel and exposed
polling starvation during large catalogue/setup inspections. NONE is accepted
as catalogue metadata but never selected as an aircraft. Independent telemetry
polling retains exact process, installation and setup checks, while cached
status metadata avoids reparsing full control catalogues. Native source guards
still reject changed files, aliases and port contracts before delivery.

The test restores the installation override, profile selection and selection
mode, deletes its temporary profile, stops its owned process and leaves Cockpit
stopped. Evidence and a screenshot are retained locally under
`.research/dcs-aircraft-catalogues-native/`; cleanup reported no errors.

Additional checks:

- `pnpm check`: no errors or warnings.
- `pnpm ui:build`: frontend and browser companion production builds pass.
- `pnpm profile-vehicles:test`: 20 passes.
- `pnpm profile-workspace:test`: 149 passes.
- Provider settings, provider UI and inventory tests: 33 passes.
- Portable profile library: 125 passes.
- Native DCS suite: 106 passes, three explicit opt-in/archive skips.
- Native resource delivery: 14 passes.
- Native game readers: 15 passes.
- Native passive vehicles: eight passes, including slow metadata work, stale
  exports and changed process, installation or setup identity.
- Native game context runtime: ten passes.
- Provider cache and source guard regressions pass, including expired caches,
  non-self-named module aliases and changed catalogue files.

Run the live acceptance with `OSG_DCS_VEHICLES_NATIVE=1` and
`pnpm profiles:dcs:aircraft:native` against a running dev Cockpit with CDP enabled
and its real DCS-BIOS integration already set up. The test builds its owned
process fixture with Windows PowerShell when needed.
