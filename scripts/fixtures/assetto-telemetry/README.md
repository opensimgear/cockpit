# Native Assetto telemetry acceptance

`tests/ui/assetto-native.mjs` compiles this authored C# publisher with the Windows
.NET Framework compiler into private `.research/assetto-native` directories. Each
process has the selected game's executable basename, publishes bounded pages and
exits on its private stop file or after three minutes. A ready/error handshake
reports initialization rather than assuming process creation means mapping
availability. No simulator files change.

Start the native dev cockpit with
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`, then run
`node --test tests/ui/assetto-native.mjs` with `OSG_ASSETTO_NATIVE=1` and, if needed,
`OSG_PLAYWRIGHT_MODULE` pointing at an installed Playwright package.

The acceptance uses actual native Cockpit IPC, process enumeration and creation
epochs, read-only Windows mapping acquisition, setup persistence, vehicle
observation, profile catalogue ports and rendered UI. Original manual installation
selections are restored after each case. Run without a real Assetto game open
because AC/ACC/Rally intentionally reject competing shared-memory producers.
Mappings retained by third-party telemetry consumers make the fixture fail with
an initialization error; the fixture never overwrites an existing mapping.

These are authored 4500 RPM, second-gear, 123.5 km/h samples with a 9000 RPM limit.
They establish acquisition and integration behavior, not compatibility with an
installed simulator build, driving controls or physical hardware.
