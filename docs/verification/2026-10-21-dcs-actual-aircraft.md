# Actual DCS Hornet and AH-6J verification

Actual aircraft discovery, separate catalogues, live telemetry, representative
aircraft-specific lighting settings and the Hornet → AH-6J → Hornet round trip
passed. Live testing exposed two native activation defects, which were fixed,
rebuilt and retested in the running dev Cockpit against actual DCS.

The running native dev Cockpit was exercised on Windows against the installed
Steam DCS World 2.9.30.28738 and DCS-BIOS 0.11.7. These checks used actual DCS
missions, native gateway IPC, installed catalogues, native resource assignments,
profile compilation and runtime inspection. No fixture process, authored export
packets or direct test UDP commands were used. The user selected the aircraft in
DCS because the computer-use helper could not control the simulator menus.

The saved acceptance profile contains Base CommonData altitude and a marker
value 100, a `FA-18C_hornet` tab with marker 1818, and an `AH-6J` tab with marker
606. Aircraft resources and their processing nodes belong to their respective
tabs. Lighting commands use constant values through explicit Set on change
nodes and the native game action ports. Original lighting telemetry was recorded
before either command was enabled.

## Verified actual simulator behavior

| Check | Hornet | AH-6J |
| --- | --- | --- |
| Freshly exported aircraft ID | `FA-18C_hornet` | `AH-6J` |
| Automatically selected DCS-BIOS module | `FA-18C_hornet` | `AH-6J` |
| Catalogue controls / telemetry ports | 571 / 533 | 205 / 190 |
| Representative control | `POSITION_DIMMER` | `MPS_PANELSDIM` |
| Original → configured exported value | 0 → 65535 | 0 → 65535 |
| Reported Base altitude during capture | 49 ft | 34 ft |
| Active marker values | Base 100, Hornet 1818 | Base 100, AH-6J 606 |

The Hornet catalogue excludes `MPS_PANELSDIM`; the AH-6J catalogue excludes
`POSITION_DIMMER`. Each successful applied topology contains Base and its loaded
aircraft's nodes, with all other aircraft nodes absent. Configured topologies
also contain only the matching aircraft's desired-value and setter nodes.

Lighting feedback was read independently from the native output channel
(`channel:POSITION_DIMMER:0` or `channel:MPS_PANELSDIM:0`). The requested action
pack on the separate command input was also inspected. Thus the value checks
compare actual simulator exports against the requested setting, rather than
checking the request alone. This qualifies these representative lighting
controls and telemetry paths; it does not exercise every advertised control.

Two read-only Hornet captures 50 seconds apart retained known Base and aircraft
telemetry beyond the configured 1500 ms freshness window, with native inspection
observation time advancing by approximately 50 seconds. This verifies continuing
delivery while stationary, without treating native observation timestamps as
raw UDP packet timestamps. All successful captures retained the original
provider setup revision and connection settings while the active module changed.

## Automatic switch and fixes found by live testing

The first actual Hornet → menus → AH-6J switch retained profile document
revision 3 throughout. Native owner generations changed from 17 (Base + Hornet)
to 18 (Base only while no aircraft was loaded), then 19 (Base + AH-6J). The
aircraft/module selection and graph change occurred without another Apply or
profile mutation. AH-6J lighting was subsequently configured at revision 4,
owner generation 20, with returned native brightness 65535.

The watch also recorded a 26-second interruption after detecting AH-6J. During
preparation, status became degraded and reported `Profile preparation was
cancelled by a newer native control`; the native reducer was unavailable for
inspection until recovery. Only read-only watch and capture requests were sent
during that transition. A later save changing an inactive Hornet setting also
reported cancellation while the previous applied AH-6J revision remained active.
The present disconnected Pico gateway on COM4 was eligible for the renderer's
automatic reconnect retry. Such requests were correctly rejected while a
profile owned the runtime, but incorrectly invalidated its preparation epoch
before rejection. Rejected independent writes now return before invalidation;
the existing dispatch ownership recheck and explicit recovery exception remain.

A subsequent live save exposed a separate expired-baseline failure. The game
reader timestamped newly received exports before slow catalogue/process
validation, and catalogue publication could further age the baseline. Acquisition
now samples the native source clock after validation, immediately before reading
actual telemetry. Activation reacquires a complete baseline after publication.
The original 1500 ms freshness policy, advancing-counter requirement, process and
aircraft identity checks, command owner/sequence checks and one-shot restrictions
remain in place. The expired-baseline failure compensated owned effects; the
test explicitly recovered/stopped profiles before each native rebuild.

With both fixes running, revision 6 activated against actual AH-6J. Saving an
inactive Hornet setting of 43690 produced revision 7 and `saveApplication: applied`.
A real Pico reconnect probe was rejected with the existing ownership error, and
the save completed successfully. The capture at 07:27:41 UTC shows Base altitude
34 ft, markers 100/606, actual AH-6J brightness still 65535, and no Hornet nodes.
Restoring AH-6J brightness to its original 0 saved/applied revision 8; independent
live feedback confirmed 0 at 07:28:02 UTC. A subsequent four-minute native watch
retained the same running owner with no degradation or inspection failures.

## Return switch and cleanup

After the fixes, the actual AH-6J → menus → Hornet return retained revision 8
without another Apply or profile mutation. The native watch recorded owner
generation 3 (AH-6J), generation 4 (Base alone at 07:35:01 UTC), then generation 5
(Base + Hornet at 07:35:51 UTC). Fresh Hornet detection at 07:35:40 began the
guarded preparation; it completed automatically in approximately 11 seconds.
The intermediate preparing/degraded state is expected while the outgoing
aircraft reducer is stopped. No cancellation, expired-baseline failure or
recovery fault occurred on this return.

At 07:35:55 UTC actual `POSITION_DIMMER` feedback was 43690, matching the value
saved while Hornet was inactive. Base altitude was 49 ft, markers were 100/1818,
the module was `FA-18C_hornet`, and every AH-6J node was absent. A repeat capture
16 seconds later retained the same owner and live values. Inspecting the retired
AH-6J owner was rejected as expired. Provider setup and connection settings
remained unchanged throughout.

Hornet brightness was restored through its own tab at revision 9, with actual
feedback 0 verified at 07:36:39 UTC. Both aircraft's originally observed lighting
positions have therefore been restored and independently verified.

The acceptance profile was stopped, the original Default profile selection and
Manual mode were restored, and final native status confirmed stopped with no
owner or fault. Dev Cockpit and actual DCS remain open. The saved acceptance
profile is retained for inspection, with both lighting constants restored to 0.

Regression coverage includes a paused Save preparation receiving rejected
background reconnect requests, and a deterministic two-second native validation
delay before real UDP acquisition. The latter also verifies that silence, frozen
counters and changed catalogues still invalidate the source. Automated tests use
isolated fixtures; the simulator captures above use actual DCS exports.

Validation:

- Native dev binary build passed and that rebuilt binary ran the final live
  saves, return switch and restoration.
- All 50 focused native regressions passed across targeted runs: 16 game-source,
  15 host-launch, one lease/baseline/Stop, five recovery, 11 editor-save and two
  automatic vehicle-switch tests.
- The automatic vehicle fixtures now provide private DCS setup/catalogues,
  preserving their original switching assertions. The newer-save test changes
  editor state without depending on real game discovery or renaming Default.
- `git diff --check` passed.

Local evidence is retained under `.research/dcs-actual-aircraft/`, including
timestamped captures, native watch JSONL, the saved graph and exact assignments,
and independently reviewed Hornet and AH-6J proof summaries. The acceptance
profile is retained for review.
