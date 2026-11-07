# Companion firmware release observation

The release resolver retains a separate `companion.firmware-releases`
snapshot. Successful catalog checks, catalog errors, and changed installation
results publish the same metadata shape as `companion_status`. Repeating the
same result preserves the revision. Registration and publication use the
resolver's state lock. Downloads and firmware validation happen outside that
lock. The existing `companion.firmware` topic continues to report USB flash
progress.

Companions shares one visible release reader across the list and details. A
release observation takes precedence over release metadata in another command's
response. Native, browser-dev and MCP consume the same retained snapshot.

## Running Cockpit

`tests/ui/companion-firmware-release-observation.mjs` passed in the native
Windows WebView and Edge browser-dev. It authored a previously paired Sunton
row in the portal's status response and pinned that response's release fields
to their old values. The view initially showed Unknown, then updated to 2.3.4
with zero extra `companion_status` reads. Leaving Companions retired the exact
reader. A check committed 2.3.5 while closed with zero status reads or new
subscriptions. Reopening displayed 2.3.5 and the second reader also retired.

The native fixture used a temporary loopback MCP credential. The SDK listed
the resource, saw its unchecked baseline, and triggered the first change via
`companion_firmware_check`. Resource reads agreed with the portal on both
versions. A second check of 2.3.4 kept the exact revision. Cleanup revoked the
credential and restored the MCP listener setting.

The dev binaries selected a bounded local JSON catalog in a scratch copy of
`release_catalog` when `OSG_TEST_RELEASE_CATALOG` was set. The same opt-in made
the resolver select its published-release projection instead of its embedded
development projection. The catalog ran through the production parser,
commit, stream, gateway and UI paths. The scratch override was removed before
the production-source native suite. The fixture used metadata only. Physical
USB firmware acceptance remains in [#199](https://github.com/opensimgear/cockpit/issues/199).

Local logs are `live-native-mcp.log` and `live-browser-final.log` under
`.research/reactive/firmware-release-observation/`. The desktop executable
SHA-256 was `0CBA82656FCD9B163BB46330ED9EE65AF3E721FA07EFC4447AF5A13B5ED771AA`.
The browser-dev gateway SHA-256 was
`98CFCF6A3B39695219D360E49751A4D64471FA641D5B07D65B02F86FFD890360`.

The final registration guard rejects duplicate registration before creating a
second topic. The exact production-source desktop candidate separately passed
the retained development baseline check with the local catalog override absent.
The snapshot reported both supported boards and agreed field for field with
`companion_status`. See `live-production-baseline.log`. The executable SHA-256
was `DC15306FC64F8B24E4F3838B187C8E665C5E1B19FE0B67B679BE865BDB8D8A8A`.

The isolated frontend passed all 1,699 tests, Svelte checking, and the Vite
production build. The shared visibility helper reached 100% lines, branches
and functions.

The Windows native suite passed 2,403 tests with 18 default ignores. Both
pinned DCS smokes and the opt-in native qualification check passed separately.
All six measured production line gates passed with a minimum of 72.92%.
The release resolver and new observer reached 71.11% and 100% LLVM function
coverage respectively. The broader companion host, commands and existing USB
firmware observer modules measured 67.50%, 56.25% and 66.67% function coverage.
The native module gates use production lines. The compiler emitted no branch
counters for these modules.
