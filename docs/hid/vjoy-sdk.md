# vJoy SDK receive boundary

The debugger qualifies the Windows x64 Brunner vJoy 2.2.2.0 release. It loads
`C:\Program Files\vJoy\x64\vJoyInterface.dll` through an explicit DLL search
policy, hashes that file and `C:\Windows\System32\drivers\vjoy.sys`, retains
read handles denying replacement, and requires matching active DLL/driver API
versions `0x222`. The hashes match the existing virtual-controller qualification:

| Artifact | SHA-256 |
| --- | --- |
| x64 DLL | `199316aaefac95118e869c07e7f332ce492dd9a8b6dac5a3ca20b1d5bb5058a1` |
| Driver | `540ac46bc529001798068bd402bb4c36891d4c0e63b9dac9761b14bd38667d7d` |

The source contract is pinned to
[`bd268d1fa5934a900a79e76e519d4aa804e0ee3d`](https://github.com/BrunnerInnovation/vJoy/tree/bd268d1fa5934a900a79e76e519d4aa804e0ee3d):
the [SDK header](https://github.com/BrunnerInnovation/vJoy/blob/bd268d1fa5934a900a79e76e519d4aa804e0ee3d/SDK/inc/vjoyinterface.h),
[interface implementation](https://github.com/BrunnerInnovation/vJoy/blob/bd268d1fa5934a900a79e76e519d4aa804e0ee3d/apps/common/vJoyInterface/vJoyInterface.cpp),
[public structures](https://github.com/BrunnerInnovation/vJoy/blob/bd268d1fa5934a900a79e76e519d4aa804e0ee3d/inc/public.h),
and [FFB descriptor](https://github.com/BrunnerInnovation/vJoy/blob/bd268d1fa5934a900a79e76e519d4aa804e0ee3d/apps/vJoyConf/hidReportDescFfb.h).

Inspection reports all sixteen SDK axes and their logical ranges, button and
both POV counts, all twelve effect capability usages, driver FFB support,
installed/max device counts, existence, ownership and the complete V3 field
layout. Missing and unknown slots retain null capabilities. A negative axis
error fails inspection except `NO_SUCH_AXIS = -10`. SDK boolean effect queries
cannot distinguish an unsupported effect from every internal query failure.
The SDK owner query itself reads a 16-bit PID in this release; its reported PID
can therefore be truncated on systems with larger process IDs.

`configuredDescriptor` separately preserves the selected slot's persisted
registry descriptor bytes, SHA-256 and parsed per-kind/report-ID bit and byte
lengths. Its provenance explicitly leaves active-driver correlation unverified;
an absent entry can mean the implicit default layout. Reads are repeated to
detect concurrent configuration changes. Input is capped at 65,536 bytes and
the JSON evidence at 168,000 bytes. If the parsed details exceed the budget,
the result preserves the exact descriptor bytes, digest and counts and explains
why the expanded report rows and diagnostics are unavailable.
The helper transport itself accepts at most 512 KiB per message.

`JOYSTICK_POSITION_V3` has size 124, alignment 4, one device byte at offset 0,
three padding bytes and thirty 32-bit fields beginning at offset 4. No position
report is submitted by the debugger. Native `FFB_DATA` has x64 size 16 and
alignment 8: 32-bit size at offset 0, 32-bit command at offset 4 and pointer at
offset 8. Its size field instead counts an eight-byte **wire** header plus the
payload. The callback rejects null pointers and wire sizes outside 9–256, then
copies `size - 8` bytes before returning. Payload byte 0 includes both the slot
in its upper nibble and report type in its lower nibble. Receive therefore
supports slots 1–15; slot 16 remains inspectable.

Each inspection or capture starts a separate helper process. Capture requires
a FREE, FFB-capable slot, rechecks ownership immediately before acquisition,
and relinquishes only the slot it acquired. The profile/output service excludes
conflicting ownership before opening capture; competing external feeders are
also rejected by the native device open. Capture acquires the vJoy feeder
handle because the SDK offers no passive FFB subscription. It receives the
write/set-feature requests the driver exposes to that owner. It does not
observe another feeder, submit position reports, provision slots, forward
effects to physical hardware, or measure physical force.

The SDK starts receive automatically during acquisition. Its deprecated start
and stop functions do not control the current receive lifecycle. Its callback,
userdata and packet buffer are DLL-global, and its window/receive threads have
no join API. Consequently the helper registers a process-lifetime callback
with null userdata and retains the loaded module until process exit. The
callback performs a bounded, nonblocking send of owned bytes; queue overflow
ends capture explicitly. User event sinks never run on the SDK callback thread.
The parent dispatches its sink from a dedicated reader and joins it during
close. Closing stdin detaches callback routing and relinquishes the handle;
after a bounded grace period the parent terminates a stuck helper. Every reopen
has a new process, preventing a delayed callback from entering a new capture.
During idle receive the helper checks retained ownership every 500 ms so driver
removal or lost ownership becomes an explicit backend error.

For Create New Effect, the driver overwrites the third payload byte with the
allocated block ID before notifying the feeder. Zero reports allocation
failure. A capture preserves those callback bytes. The original host request
and the driver's feature-read responses are separate observations.
[Pinned driver packet processing](https://github.com/BrunnerInnovation/vJoy/blob/bd268d1fa5934a900a79e76e519d4aa804e0ee3d/driver/sys/hid.c#L2879-L2928)

Focused tests cover native sizes/offsets, copied memory lifetime, malformed
pointers/lengths, exact capability/error handling, process handshakes and
message bounds, backend failure, graceful/forced cleanup and exclusive capture
lifetime. A read-only installed-SDK test runs when the DLL is present; it needs
normal host driver access. Independent ctypes fixtures compare decoded values
against the pinned DLL's pure `Ffb_h_*` helpers. Receiving real simulator FFB
still requires an already configured FFB slot and a producer sending effects.

## Windows qualification, 28 November 2026

The running Cockpit and an independent SDK probe agreed on three free slots.
Slots 1, 2 and 3 exposed respectively 16, 2 and 3 axes, with 8, 8 and 16
buttons. All reported force feedback disabled. The debugger refused capture;
the independent inventories before and after acceptance matched.

The Debug view and authenticated MCP replayed seven lifecycle callbacks and
17 report-layout cases from the authored corpus. The pinned DLL's pure decoder
helpers independently verified 18 comparisons. These tests exercised
decoding and replay; live simulator callback traffic was unavailable on this host.
