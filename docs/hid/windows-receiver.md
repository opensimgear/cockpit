# Windows HID receiver

The native cockpit exposes all present HID top-level collection interfaces through its
controller gateway. The profile mapping inventory continues to list mapping-compatible
controllers. Generic HID discovery uses SetupAPI and opens each interface with zero desired
access and shared read/write permissions to obtain its attributes and preparsed capabilities.
Denied metadata, failed stages and truncated enumeration appear in the response. No optional
driver is required. [Microsoft's HID collection lifecycle](https://learn.microsoft.com/windows-hardware/drivers/hid/opening-hid-collections)

## Inspecting and receiving

The following names use the common `controller_hid_` prefix. Responses are under
`state.controllers` in the gateway envelope. Only `open` starts a receiver.

| Command | Request | Result |
| --- | --- | --- |
| `list` | No target | Interface IDs, usage, product and access evidence. An unprobed interface still requires a successful shared read open. |
| `inspect` | `interfaceId` from the current discovery list | Metadata and Input, Output and Feature caps. Inspection performs no report GET. |
| `open` | `interfaceId` | Starts an ordered input stream for the selected collection and returns its `generation`. |
| `reports` | `interfaceId`, `generation`, optional decimal-string `after` sequence | Retained reports after the cursor, oldest/latest sequence, gap flag and terminal error if present. |
| `get` | `interfaceId`, `generation`, `requestId`, `kind` of input or feature, numeric `reportId`, `maxBytes`, `deadlineMs` from 1 to 2000 | A single explicit read of an advertised report, with returned bytes or a read error. Feature GETs may have device-specific effects. |
| `close` | `interfaceId`, `generation` | Releases the selected reader. Reopening produces a new generation, including after disconnect. |

Each selected reader uses a shared, read-only Win32 handle. Keyboard, mouse and restricted
system collections expose metadata. OS permission and sharing failures remain visible when
other collections are opened. A Windows input delivery includes an ID byte, zero for an
unnumbered delivery. Every successful stream delivery receives its own sequence and monotonic
timestamp measured from capture start. Equal payloads are separate events. The bounded
history retains up to 4,096 reports or 8 MiB of encoded records per reader, with at most
four selected readers. It reports a gap when a cursor
falls behind. These cursors can identify loss in the cockpit buffer only.

## Interpreting the evidence

HidP supplies per-type maximum byte lengths, collection links, aliases, usage/data-index
ranges and cap flags. Value caps also provide widths, counts, bounds, units and exponents.
The receiver interprets `HidP_GetData` indices against the matching kind and report ID,
and calls `HidP_GetUsageValueArray` for supported usage-value arrays. The latter returns
packed bytes for that array. An unknown index, usage or unprojectable field can still be
investigated using the original delivery. Relative samples remain deltas.
[Microsoft's HID report interpretation guide](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/interpreting-hid-reports)

The snapshot includes the host HID parser API version and how it was determined.
Button report counts are available starting with HID parser API version 2. Earlier or
unavailable versions return a null button count. Value-cap report counts come directly
from the value capability record. [Microsoft's button capability structure](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidpi/ns-hidpi-_hidp_button_caps)

Preparsed data is opaque. A cap index, Windows link index or data index does not establish
an original HID item offset or report bit offset. Per-kind maximum buffer lengths also
do not establish individual report-ID payload lengths. The raw descriptor, original item
order, global Push/Pop and exact padding positions remain unavailable through this path.
[Microsoft's preparsed data documentation](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/obtaining-preparsed-data)

## Hardware check

With the native dev cockpit exposing its local debug endpoint and the two VKB fixture
controllers connected, run `node tests/hid-device-captures/verify-windows-receiver.mjs`.
It checks the live capabilities against the archived collection, report ID and data-index
fixtures, then compares current report bytes from the cockpit to an independent Windows
Raw Input capture. The test requests input only. Native-boundary tests cover feature GET,
multiple IDs and failure paths without issuing feature requests to physical hardware.
