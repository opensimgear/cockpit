# Normal development Cockpit: combined Windows rig

This records an actual run of the normal development Cockpit on main `8fe5eb2`,
using the explicit Windows debug build option `OSG_DEV_NATIVE_PROFILES=1`.
It used the existing managed native application and ordinary Profiles UI, not a private qualification executable or a second GatewayManager.

## Saved graph and first activation

The saved profile was `697b4747-29d4-4dbd-83a8-ce0480e206b6`, revision **5**,
“MH-6J - combined VKBs, hidden pedals and browser”, with actual DCS MH-6J context.
The graph combined right VKB X/Y and left VKB Y into vJoy slot 3 X/Y/Z, routed
both VKB POV1 controls to virtual hats 1/2, and requested hiding only the pedals.
The sixteen reserved virtual buttons stayed false; no physical button coverage
is claimed. Actual CommonData altitude fed declared feet, conversion to metres,
unconditional Text, and a 3000/2900-metre hysteresis Indicator.

One ordinary UI Apply reached Running, session
`5fb4c81b-9238-4f90-a32f-b92af83874ef`, generation **1**, revision **5**, game `dcs`.
The measured phase entry times were Overrides 0 ms, Acquire 243 ms, Baseline
1852 ms, Visibility 55 ms, and Start 183 ms; Start authority publication was 0 ms.
`ui-apply-revision-5.json` retains the actual native UI result.

## Independent consumer, hiding and browser feedback

An independent DirectInput game consumer read exact vJoy slot 3 instance
`24386ff0-c8aa-11f1-8002-444553540000` with 3 axes, 16 buttons and 2 POVs.
During the bounded 120-second human movement observation it made **5911 reads**
and recorded **371 changes**. X, Y and Z each reached the consumer's full default
range 0–65535. Each POV separately reported neutral, 0 and 18000, and both were
neutral at the last recorded change. This proves both virtual hats were visible
to an actual consumer; it does not claim every hat direction was exercised.

The consumer made one background/nonexclusive DirectInput acquisition and successfully
Unacquired, released both COM objects, and destroyed its unshown window. It made
**zero vJoy feeder acquisitions, device-property writes or policy writes**.
The runtime's unchanged feeder owned the output.

Independent DirectInput enumeration while running found both VKBs and vJoy 3,
with the selected T-Rudder absent. After explicit Stop the pedals were visible
again. HidHideCLI could not open the driver while the profile was active
(AccessDenied), so this record does not claim a direct active policy dump.
The stopped CLI readback exactly matched the capture-ready baseline: three
foreign hidden entries, five application exceptions, and unchanged active and
inverse state. The comparison preserved foreign entries rather than clearing
the policy. Stop UI evidence records Stopped and no current owner.

The same canonical browser session `66270`, evidence directory
`browser-18c44f3cb5a34fc79a4326e5bae3b790`, remained paired. Observations 2/3/4
rendered known `ALT 159.7 m` and Indicator `Inactive`, with native `local · host`
provenance and fresh ages 78/218/79 ms. All had zero alerts and page errors.
After Stop, observation 5 showed Text and Indicator `Unknown / No source`, while
the browser remained paired and active through a real reconnect; no re-pair
was used. Unbound Numeric and Gauge widgets correctly remained No source.

## Same-revision reapply

After Stop/restoration, one normal UI Apply of the **same saved revision 5**
reached Running in the same session, generation **2**, game `dcs`, with no pending
transition. The saved semantic hash remained `3984dc6b…`. No pairing, assignment,
reconciliation or Save was performed between Stop and reapply.
Phase entry times were Acquire 253 ms, Baseline 1539 ms, Visibility 93 ms and
Start 95 ms; authority publication again took 0 ms.

The same browser/client rendered observation 6 as `ALT 159.7 m` / Inactive,
fresh 243 ms, then observation 7 as **`ALT 160.0 m`** / Inactive, fresh 216 ms,
with known native provenance and zero alerts/errors. The actual numeric value
advanced. Fresh generation-2 DirectInput identity inspection again found both
VKBs and vJoy 3 present and the pedals absent. The latest retained native
readback at 12:43:16 UTC was still Running generation 2; no final generation-2
Stop/restoration is claimed here.

## Source fixes and retained evidence

`6e9ce5e` introduced the explicit Windows-debug development acceptance option; default/release/non-Windows qualification gates and the script gate remain unchanged.
`8e47b55` separated visible browser feedback from focused input, retaining fresh neutral reconnects and blur/hidden input cancellation.
`e9de906` moved duplicate visibility reads to the existing worker and preserved
neutral-output renewal. `772cb22` waits for actual background companion inventory
and exact current generations, including feedback-only graphs. `8fe5eb2`
publishes game ownership without synchronous rediscovery; direct admission still
checks process, setup and catalogue identity. No lease duration was extended.
Focused before/after proofs are retained in `.research/p9-visibility-start/`
and `.research/p9-context-owner/`; browser lifecycle proof is in
`.research/p9-browser-focus-lifecycle/`. These are separate from actual rig proof.

Private evidence under `.research/full-flow-dev-20261018/` is indexed by
`acceptance-manifest.json` (`eb861fec7f5aca137d6e93d86aed0d4ec853048813c1db9fd8760bfa5f6839de`):

| File | SHA-256 |
| --- | --- |
| `ui-apply-revision-5.json` | `aaa762fa49afd07c46934f67e5c25e830ee04e928559e5332e3af2462c5784fd` |
| `directinput-vjoy-live-motion-r5.json` | `2fc761d839cc1c073e4aa9594769034466b6710656066242623f678f209a1055` |
| `ui-stop-after-combined-motion-revision-5.json` | `dbca58caf9e31ba1f1307656f539909d41ff9d7f1cffdef930f9d6458e28d77b` |
| `ui-reapply-same-revision-5-generation-2.json` | `3e6ec1162c0f2f980c62e69d2ee0872a71009f1533bab83d8ac303371a5d7e89` |

This records only the observed selected development flow. No actual MH-6J control-surface visual response,
all-button behavior, threshold crossing, other OS/layout qualification or release-gate graduation is asserted.
Actual OSG hardware was explicitly deferred because the user has none. Actual Devices and separate Topology
views still showed the capture-host VKBs and pedals Connected, without hidden-by-profile badges,
generated-vJoy associations or profile-name modifiers. That separate Devices #78 scope is not completed by this runtime proof; evidence is
`ui-devices-topology-active-revision-5.json` (`bfebb0dd56d9c1ae30bd9e4ff5b463d417418943f1c25419f57efc6b4c533d77`).
