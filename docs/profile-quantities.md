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
