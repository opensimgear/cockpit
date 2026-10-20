# Assetto Corsa Rally prefix fixtures

## Development status and disclaimer

This project is heavily under development and is not ready for use. Use it entirely at your own risk. To the fullest extent permitted by applicable law, the authors and contributors disclaim all liability for any damage, loss, or other issues arising from its use.

These authored binary samples exercise the production ACR parser and source
freshness engine. They are synthetic; they are not recordings or evidence of live
game, Windows process or hardware qualification.

The field order follows the ACR-specific developer's MIT-licensed
[physics declaration](https://github.com/LuizZak/AssettoCorsaRallyTelemetryReader/blob/8bb26990def38ffd0e4afe71e04e85e5ddeb3670/ACRallyMemReader/Data/Models/Physics.cs)
and [static declaration](https://github.com/LuizZak/AssettoCorsaRallyTelemetryReader/blob/8bb26990def38ffd0e4afe71e04e85e5ddeb3670/ACRallyMemReader/Data/Models/StaticInfo.cs).
They use explicit little-endian 32-bit numbers, Windows UTF-16 and Pack=4.
`manifest.json` records sizes, offsets and expected values. Prefixes require
32 physics bytes, 8 graphics bytes and 416 static bytes; these are not full-page
size assertions. These legacy fixture lengths and manifest remain intentional.
The parser also accepts an optional 592-byte physics prefix, reading
`currentMaxRpm` as little-endian int32 at offset 588. A valid current maximum
takes precedence over the legacy integer at static offset 412; a missing,
truncated, zero or invalid extension falls back to a valid static maximum.
Neither source is carried over from a previous sample or car. See the
[live correction](../../../docs/verification/2026-10-21-acr-current-max-rpm.md).
The graphics status enum comes from the independent ACR recorder's ACC-compatible
reader; pause/replay behavior remains to be qualified on each live build.

Fixtures cover source progression, duplicate/frozen updates, reset,
reverse/neutral, invalid speed/RPM, short data, paused/replay/off/unknown status
and absent maximum RPM. Production tests additionally cover the optional current
maximum, its static fallback and both-source absence, plus torn copies,
malformed UTF-16, source/context replacement, u32 wrap and stale duplicates.
Gear normalization `raw-1` is developer-derived; original AC UDP telemetry is a
separate protocol and is not used here. A missing maximum yields unknown
`max_rpm` and `rev_ratio`; no rev limit is invented.

The Windows source opens existing `Local\acpmf_*` mappings read-only. It binds
an explicitly selected process ID/creation epoch, checks the executable image,
closes on exit/reset and performs bounded repeated-copy checks. The default
image is the developer-reported `acr.exe`; `new_for_image` supports an explicitly
configured alternative executable basename. Mapping names are shared by several
Assetto titles and do not expose their creator PID. Run the selected title alone;
matching the selected image cannot prove ownership of a competing game's named
mapping. No atomic multi-page publication guarantee was found.

Simulation input `{sourceId?,sourceEpoch,physics,graphics,staticPage}` uses bounded
byte arrays and the same parser as Windows acquisition. Source epochs use decimal
strings in JSON to preserve u64 process creation identities. A fixture queue can
carry at most256 snapshots. First attachment, game/context change, reset and
resumption need a baseline then advancement of the original physics packet ID.
Duplicated copies or relay reception timestamps never renew freshness.
