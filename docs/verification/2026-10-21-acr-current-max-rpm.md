# ACR current maximum RPM correction — 2026-10-21

Cockpit previously read maximum RPM only from the legacy static page at offset
412. That slot remained zero in the running game, so `max_rpm` and `rev_ratio`
correctly stayed unknown and the Sunton percentage gauge had no usable input.
The game publishes its current maximum in the physics page instead.

## Live capture

A read-only capture at `2026-10-20T23:44:35Z` (21 October locally) inspected the
existing `Local\acpmf_*` mappings for the selected ACR process, PID 62620:

| Item | Observed value |
| --- | --- |
| Executable | `acr\Binaries\Win64\acr.exe` in the Steam ACR installation |
| Executable SHA-256 | `42c359b0cd24774a86441710fdc7d7db3bc06b57b3ff30cb9ea5d56fb71448fb` |
| PE file/product version | `UE5-CL-0` / `UE5-CL-0` |
| Car / track | Lancia Stratos HF / Wales Afon Bidno |
| Physics packet IDs | 1707979 through 1708281, ten advancing samples |
| Engine RPM | 1436 |
| Static maximum, byte 412 int32 | 0 |
| Physics current maximum, byte 588 int32 | 8500 in all ten samples |
| Same physics bytes decoded as float32 | approximately `1.1911e-41` |

The bytes at physics offsets 588–591 were `34 21 00 00`. The corresponding
ratio is `1436 / 8500 = 0.168941…`, or approximately `16.8941%` after the existing
clamped ratio-to-percentage transformation. Raw prefixes and acquisition
provenance are retained locally in `.research/acr-redline-raw.json`; that ignored
diagnostic capture is not a synthetic fixture or a committed test artifact.

## Layout evidence and behavior

The [pinned ACR reader](https://github.com/albertowd/live-telemetry-evo/blob/0dd9992cde4d599d8f914300957eb16955b05804/src/live_telemetry_evo/sources/acrally.py)
reports sparse static data and reads `currentMaxRpm` as int32. Its
[layout probe](https://github.com/albertowd/live-telemetry-evo/blob/develop/tools/probe_rally_layout.py)
identifies offset 588. These are primary community implementation sources,
not an official ACR layout guarantee. The live capture above resolves the
integer-versus-float ambiguity for this executable fingerprint.

The parser retains the required 32-byte physics prefix for legacy samples and
reads the optional current maximum only when at least 592 bytes are available.
A valid physics current maximum takes priority; absent or invalid current data
falls back to a valid static maximum. Both obey the existing 1–100000 RPM
contract. If neither is valid, maximum and ratio remain unknown. No observed
peak, guessed redline or previous-car value substitutes for published data.
Source freshness, context resets and pause/replay handling still gate output.

## Native verification

The read-only live Windows acquisition test passed against the running game:
fresh RPM 1436, maximum 8500 and ratio `0.16894117647058823`. Parser/session
tests passed (12), as did ACR setup/acquisition tests (33 passed, two opt-in
tests ignored). The actual live acquisition was then run explicitly and passed.

After rebuilding, native dev Cockpit PID 38440 activated the existing saved ACR
profile `45ae8d0e-2413-445f-a3ff-822139710f50`, revision 13. The owner was
session `b5668ce5-3c48-4550-88dc-c8a187463b95`, generation 1, with state
`running` and no degradation. The saved ratio-to-percentage transformation
retained clamping to 0–100%; the existing speed route was unchanged.

Twelve owner-bound inspections each compared the source ratio and gauge output
from the same native frame. Every output matched `clamp(ratio * 100, 0, 100)`.
Observed percentages were 92.3294, 83.5647, 57.5882, 78.8588, 99.8353, 86.2,
86.8471, 77.2235, 54.5059, 25.4941, 20.3059 and 17. Sunton remained connected
and armed, reported its panel operation ready, and had zero USB write failures.
Local evidence is retained in `.research/acr-redline-verified.json` and `.png`.

This verifies native acquisition, the active profile's changing gauge values and
healthy Sunton transport on the captured executable. Existing diagnostics do not
expose a firmware acknowledgement of each individual gauge value, and physical
screen pixels were not observed. Other ACR versions and every lifecycle
transition have not been qualified by this check.
