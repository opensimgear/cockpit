# Windows virtual-controller latency during an inactive profile save

Running Windows dev Cockpit routed two native debug input lanes to separate
buttons on one vJoy slot. Their measured rates were 1,000.02 and 125.00 Hz in
the second trial, with 7.20 seconds of overlap. The editor renamed and saved
a different profile from revision 1 to 2. The active run kept the same owner,
revision and generation. Each trial delivered all 8,102 ordered edges and
both Stop releases to the Windows Raw Input receiver.

| Trial and lane | Offer to receipt p50 / p95 / p99 | Maximum offer to receipt | Maximum dispatch to admission |
| --- | ---: | ---: | ---: |
| 1, 1 kHz | 0.4 / 0.6 / 7.1 ms | 60.28 ms | 57.67 ms |
| 1, 125 Hz | 0.7 / 0.8 / 2.9 ms | 57.52 ms | 56.49 ms |
| 2, 1 kHz | 0.4 / 0.6 / 1.2 ms | 51.89 ms | 50.83 ms |
| 2, 125 Hz | 0.7 / 0.8 / 3.0 ms | 51.54 ms | 49.00 ms |

The aggregate percentiles mask a repeatable stall. In trial 2, the Save click
and acknowledgement were 99.49 and 239.23 ms after the scheduled workload
start. Within that interval, padded by 2 ms on either side when aligning with
the first recorded offer, receipt p95 was 47.14 ms for 144 fast offers and
51.54 ms for 18 peer offers. The worst fast offer spent 0.02 ms before
dispatch, 50.83 ms before admission and 1.05 ms from admission to receipt.
The worst fast offer in trial 1 spent 57.67 ms from dispatch to admission
and 2.60 ms from admission to receipt.

The editor takes `profile_admission` for writing while normalizing,
validating and persisting a profile, including an inactive profile. Debug
input admission takes the same lock for reading before entering the
coordinator. The native physical controller capture path uses its own
report watch. This run covers authored debug admission and vJoy receipt.
Physical controller input, preview frames and display scanout require
separate traces. The next software check is to let debug admission proceed
during unrelated persistence while keeping owner replacement and saved
revision checks atomic, then rerun with a save-window timing assertion.

The first fixture asserted cached slot availability immediately after Stop.
The corrected second trial checked the native release separately from the
cached status. Both releases reached Raw Input within 2.55 ms of Stop and
the inventory showed the slot free by 296 ms. Cleanup reported no errors.

The native binary includes the watched-game launch change from `c8a57cb0`
and has SHA-256
`4933f3a9c9bea2da4106484d24248e39f644095676473091109b7076a7ad4050`.
The frontend is from `88ee864e`. The Raw Input probe SHA-256 is
`0a66dca7c5355cafe6e1bc12fc52e00e06af9fda9b16baa1c27eea6f44a0bee5`.
The local trial traces are in `.research/c193-reconnect/current-ui-88ee864e/source/.research/profile-save-dual-rate/`.

| Trial directory | `evidence.json` SHA-256 |
| --- | --- |
| `d89b971e-68ae-4626-bdad-cf02710568b6` | `5592a4d2ad94188004c28d93d9ae2d31fb819529b745a1fa97f3119308e38076` |
| `30b5a019-f5a4-4393-b75e-b57d93730d7b` | `4b0eef3605bc2d5ef6264ea451f24fbd429b278c1eb4189d63d20295aa50008a` |
