# Interpret raw quantities

Some providers export numeric values with a range but no physical unit. In the
profile editor, select that output and add **Interpret raw quantity**. Enter a
quantity name that identifies the measurement, such as a provider's reported MSL
altitude, and set its documented unit. The initial generic declaration is
**Raw count**, in **count** units.

The node preserves the exact numeric value, scalar type and range. If the raw
encoding needs scaling or an offset, author that calibration first using
**Map numeric range** or **Scale and offset**. Follow the quantity declaration
with **Convert units** when converting a declared unit, such as feet to metres.
Each step remains visible in the graph and applied inspection.

For example, a provider that documents one exported count per foot can use:

1. Its raw integer output, range 0–65535 and no declared unit.
2. **Interpret raw quantity**, with the documented altitude definition and `ft`.
3. **Convert units**, from `ft` to `m`.
4. A condition with hysteresis, followed by the companion's declared indicator
   input or a separate visible formatting node.

The quantity declaration is part of the captured profile program. It carries
the author's interpretation of an exported value; source quality and delivery
provenance continue to come from the provider. Unknown, stale and unavailable
inputs retain those states through the node. Event baselines stay suppressed,
and each later event retains its occurrence.

The native operation is `declare-quantity`, with bounded `quantity` and `unit`
strings. Its sole input must be a unitless integer or number. Its sole output
must have the same delivery, numeric type and bounds, with exactly the declared
unit. Quantity names and units are nonempty, have no surrounding whitespace or
control characters, and contain at most 128 UTF-8 bytes. Existing unit-bearing
ports use **Convert units**. Scale and range mapping retain their existing unit
checks.

## RPM ratio to a percentage gauge

Connect ACR's **RPM / maximum RPM** output to the companion's percentage gauge
using **Connect with transformation**, then choose **Ratio to percentage
(0–100%)**. The visible node multiplies the ratio by 100 and clamps the result
to the gauge's 0–100% range: `0.75` becomes `75`, and `1.1` becomes `100`.
It does not normalize against the source's full declared range.

**Convert units** also supports `ratio` and `%` in either direction. Its
**Clamp to output range** option explicitly limits the converted value to the
output contract before rounding. Without that option, the whole converted
source range must fit the output. Unknown, stale and unavailable telemetry
remains unknown, stale or unavailable; a missing maximum RPM is not treated as
zero.

## DCS-BIOS reported MSL altitude

In DCS game details, choose the installed aircraft catalogue. When that
aircraft's alias explicitly includes CommonData and the installed CommonData
catalogue advertises it, the profile port catalogue includes
`channel:common:CommonData:ALT_MSL_FT:0`. Its label identifies a reported value
whose measurement validity is unavailable. CommonData supplies read-only
outputs; it cannot be selected as an aircraft or used to add commands.

The port is a raw integer from 0 to 65535. Add **Interpret raw quantity** with
quantity **DCS-BIOS reported MSL altitude** and unit `ft`, then **Convert units**
from `ft` to `m`. Connect its result to an explicit comparison or hysteresis
node and the companion's declared indicator input. This identifies MSL, rather
than above-ground, altitude. Select a separate formatting node for a text input.

The [pinned upstream definition](https://github.com/DCS-Skunkworks/dcs-bios/blob/30125bd4231f87769f46bacd6050f25e2d25d1f8/Scripts/DCS-BIOS/lib/modules/common_modules/CommonData.lua)
divides simulator metres by `0.304` before flooring the exported count. Cockpit
preserves that reported value. The subsequent explicit feet-to-metres conversion
uses `0.3048`; it does not silently repair the exporter's approximation.

The exporter may retain an earlier altitude when ownship data or its export
permission is absent. It advertises no measurement-validity flag for this
quantity. A progressing stream with the selected aircraft establishes a fresh
reported export, without establishing fresh physical measurement validity or
game-confirmed output. Zero received on the wire remains a value. Before its
bytes arrive the value is unknown; stream loss or the wrong aircraft withdraws
freshness.

The reader captures the aircraft, aliases, CommonData and metadata documents.
Catalogue metadata is checked at admission and commands. Polling checks it
after each 250 ms interval and checks a full semantic digest after each
two-second interval to catch content changes with preserved file metadata.
A changed capture requires preparing a new reader; it never silently
substitutes new channel addresses.
