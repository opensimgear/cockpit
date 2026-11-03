# Native value and contributor projection

Prerequisite for #188/#192/#193. `ExportSnapshot` and native telemetry inspection
caches now retain each value together with its original contributors. This
milestone covers projection, canonical conversion and cached inspection; typed
per-port mapping envelopes and reducer/ticket admission remain open.

## Behavior

- BIOS integer outputs carry both byte captures even with a narrow mask. Strings
  carry their entire fixed-width dependency range, including bytes after NUL.
  A projection context binds the value, lease and shared byte-witness cache to
  one immutable decoded snapshot. Later snapshots cannot reuse its cached proof.
- BIOS captures remain old when a real native counter renews the continuity
  lease. The decoder's qualification domain/generation is separate from raw
  acquisition identity and composite report/byte position. Raw transport owner
  UUIDs define acquisition lifetimes; their byte-capture generation is one.
- Ownship fields retain the original pre-lock packet time and host capture
  sequence. X-Plane fields retain individual component time/sequence and resolve
  aliases to actual dataref identities; unrelated guard updates cannot refresh
  an older component.
- Assetto fields use their actual physics/graphics/static page capture. The
  decoder records whether the engine limit came from current physics or static
  fallback, so RPM ratios combine exactly RPM and the chosen maximum's proof.
- Scaling preserves proof. Magnitude and ratio projection union actual inputs,
  reject conflicting captures and reject native/fixture mixtures. A repeated
  input is deduplicated. Latched renewal preserves capture while advancing only
  the matching lease; contradictory observation times cannot renew it.
- Cached native inspection checks proof at response time. Sampled capture expiry
  becomes stale without waiting for the separate cache timeout. Latched authority
  age uses the original counter observation, without relabeling byte captures.
  JSON responses expose ordinary values only when forming external rows.
- Pure/authored decoder values explicitly lack native proof. A native source
  cannot silently recover lost evidence by switching to this fixture form.

## Bounds

One value has at most 1,024 contributors, covering the maximum BIOS string width.
One projection admits at most 1,048,576 contributor references, independently of
provider port ceilings. BIOS byte witnesses are shared within a projection, with
at most 65,536 native byte identities. Overflow clears the whole projected result
rather than publishing a valid-looking prefix. Identity strings are bounded to
512 bytes, scalar text to 16 KiB, and native validity intervals to 11 seconds
(providers retain their stricter prepared freshness settings).

Conservative retained-size accounting counts shared witness, lease, string and
reference allocations in full. Tests exercise the maximum string width and
reference budget. Existing native journals retain native snapshots; this change
does not add an unbounded queue of projected maps. New production/test modules
are below 500 lines; the Assetto parser tests were extracted from the parent.

## Verification

The isolated full native suite passes: **1,896 passed, zero failed, 17 explicitly
ignored**, in 95.74 seconds with four workers. All **114 configured native module
coverage gates** pass, plus seven changed-parent production line checks:
DCS projection 95.10%, canonical vocabulary 89.63%, inspection 96.12%, game
adapter 91.20%, game reader 88.00%, vehicle sources 83.96%, X-Plane session 99.17%.
New evidence validation is 97.46%, captured canonical projection 93.06%, BIOS
projection 100%, Assetto projection 93.83%, ownship projection 93.55%.

Production compilation passes. Tested executable SHA-256:
`78a7566d92ae5e65f248de84b5f9cdfebe0ba2a5f9d4cd3087e855696e14e29b`.
The actual running Cockpit passes the authored DCS BIOS/ownship fixture (62.47s)
and Corsa/Competizione/EVO/Rally fixture (72.54s). These exercise selected native
and canonical processing, independent inspection, expiry/withdrawal and recovery.
Both restore original installations and report no cleanup errors. They use
authored publishers and input-only processing, with no physical outputs or real
simulator qualification.

Local evidence: `.research/reactive/value-capture-native-coverage/`,
`.research/reactive/value-capture-production.log`,
`.research/dcs-physics-native/1793675200780/evidence.json` and
`.research/assetto-capture-native/1793675349138/evidence.json`.

The X-Plane running fixture remains unqualified because process-monitor WMI
access is denied before RREF starts; no UAC was requested. Its software tests
run in the full native suite. User-operated simulator/hardware acceptance and
physical preview/raw-to-virtual latency remain in #199, first in progress.

The game adapter rechecks native value validity, but its reducer envelopes still
use admission stamps. Checked journal-clock conversion, per-port continuity,
independent family recovery, portable witness tables, selective timer/script
fencing and lease-only routing semantics are the next implementation work.
