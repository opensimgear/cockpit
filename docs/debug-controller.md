# Debug controller

In the profile editor, open **Controllers** and add **Debug controller**. Configure
the button, axis and hat counts, connect its outputs, then save and apply.
The details panel has sliders, a two-axis pad, momentary buttons and hat selectors.
The axis reset returns all sliders to neutral; closing the panel releases held
buttons. The same runtime controls are available through MCP.

A debug controller is a portable profile resource with `kind: "debug-controller"`.
Its native inputs enter the same applied profile reducer as physical controls,
so mappings, transformations, virtual controller outputs and desktop bindings
can be exercised without input hardware.

The resource declares one through 128 output state ports in total. Port labels
are editable, while identities and value domains follow controller conventions:

| Identity | Schema | Neutral fallback |
| --- | --- | --- |
| `axis-1` through `axis-16` | Integer, -32768 through 32767 or 0 through 32767; unit null | 0 |
| `button-1` through `button-128` | Boolean | false |
| `hat-1` through `hat-4` | Integer, -1 through 7; unit null | -1 |

Debug resources use no machine-local resource assignment. Visibility intents and
physical runtime overrides cannot target them. Their authored document stores
control definitions only; current input values are volatile.

Apply creates a complete neutral baseline for each used debug source. All
configured controls remain visible in debug status, including unconnected
controls. Only connected source ports produce reducer frames. Stationary values
remain live while the native profile owns the source. Stop, cleanup and a new
activation clear values, streams and queued input.

## Runtime and MCP requests

Both operations require the exact current `profileSession.owner` returned by
profile status. A selected or saved profile identity is insufficient.

`profile_debug_controller_status` accepts `{owner,nodeId}` and returns current
configured controls as `values: [{portId,value}]`.

`profile_debug_controller_input` accepts:

```json
{
  "owner": "the complete current owner object",
  "nodeId": "debug-controller-1",
  "streamId": "22222222-2222-4222-8222-222222222222",
  "sequence": "1",
  "changes": [
    {"portId": "axis-1", "value": -12000},
    {"portId": "axis-2", "value": 20000}
  ]
}
```

The example owner placeholder must be replaced with the complete returned owner
object. Each stream uses a canonical lowercase UUID and positive unsigned decimal
sequence strings. A controller admits at most 16 streams per active owner.
Sequences must increase within a stream. Repeating the latest sequence with the
same changes returns `replayed: true` and emits no additional frame; conflicting
or older sequence reuse fails.

Changes validate atomically, including port identity, integer range, type and
duplicate controls. Native code supplies frame timestamps and source sequences.
The queue holds at most 256 frames and preserves every button edge. Queue-full
rejection changes neither values nor stream state; retry the same input after
delivery progresses. Reset by submitting the neutral values of all controls.

Gateway replies use `state.debugController`; MCP replies use `data`. The state
contains `owner`, `nodeId`, `values`, `streamId`, `sequence`, `replayed` and
`pendingFrames`. Status returns null stream metadata.

MCP input requires a write credential. It is an explicit streaming mutation,
using its native stream sequence fence instead of the general 128-operation
MCP ledger, and therefore accepts no `operationId`. Input updates do not persist
profile documents, activity rows or replayable commands, and do not trigger
automatic profile selection or reactivation.
