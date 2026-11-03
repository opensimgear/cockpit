# Source-scoped recovery and baseline seeding

Incremental delivery for #188/#193. Source recovery now reevaluates the affected
source's descendants instead of refreshing every source branch. An unrelated
source's baseline cannot enqueue script callbacks, reset rule state or replace
an authored pulse's original deadline. Existing batch loss-before-timer
admission remains in place.

Actual baseline evaluation explicitly seeds synchronous built-in descendants.
Seeding resets edge history and pending rule deadlines before establishing the
current level. A held baseline cannot start a toggle, pulse, macro or gesture;
a neutral baseline arms a later genuine press. Suppressing events in a script
transaction is separate and does not invoke baseline seeding. The evaluator is
extracted into a 451-line module, with its own coverage gate.

## Reproduction and verification

The pre-fix actual Cockpit reproduced the issue using two independent authored
debug-controller sources. Activating Source B enqueued a second callback on
Source A: the script counter was **2 instead of 1**. The new native regression
fails against that executable and passes against the rebuilt executable.
The portable regression also fails against the previous production reducer.

The rebuilt running Cockpit verifies one startup callback, independent B input,
one genuine A update, no callback merely from holding A, and rejection of input
after Stop. The authored pulse-deadline and script-completion/freshness fixtures
both pass. These use supported native gateway commands and clean up their
temporary profiles; they do not qualify physical controller or simulator effects.

- Portable mapping: **159 passed**, zero failed; all **17 module coverage gates**
  pass. Production line coverage: seed 100%, evaluator 94.87%, typed reducer 91.78%.
- Mapping host: **123 passed**, zero failed; all **12 module coverage gates** pass,
  including desktop dispatch, controller decoding and companion routing.
- Full native suite: **1,896 passed**, zero failed, **17 explicitly ignored**,
  in 103.78 seconds with four workers. All **114 configured native module
  coverage gates** pass, and the source-stamped evidence is retained.
- Production compilation passes. Executable SHA-256:
  `ae3d09b0eb2fdb1a0dc2ed00c573bc09eb166ec5f56c294c20d9620d96bb5c8e`.
- Authored DCS BIOS/ownship processing, withdrawal and recovery pass in the actual
  Cockpit (58.22 seconds), with the original installation restored and no cleanup
  errors. Authored Corsa, Competizione, EVO and Rally processing/frozen expiry/
  recovery also pass (72.57 seconds); all original installation lists are restored
  and cleanup errors are empty.
- All **930** tracked Rust/SQL/manifests/locks under the native application and
  crates match the isolated candidate/index after CRLF-to-LF normalization.
  Unrelated working-tree changes are excluded from this delivery.

Local evidence: `.research/reactive/baseline-mapping-coverage/`,
`baseline-host-coverage/`, `baseline-native-coverage/`,
`baseline-mapping-before.log`, `baseline-native-before.log`,
`baseline-native-after.log`, `baseline-deadlines-native.log` and
`baseline-index-source-proof.json` under `.research/reactive/`;
`.research/dcs-physics-native/1793677743113/evidence.json`.
The Assetto artifact is
`.research/assetto-capture-native/1793677821295/evidence.json`; the new callback
regression artifact is
`.research/reactive/profile-baseline-scope-native/1793677687036/evidence.json`.

## Remaining boundary

This fixes logical-source recovery. Simulator reducer envelopes still need
portable per-input witnesses, checked journal-clock conversion, independent
per-port/family continuity, native sampled/lease deadlines, lease-only admission
and retained asynchronous contributor bindings. It does not claim those native
authority semantics are implemented by source-scoped baseline evaluation.

The full reactive cutover and physical preview/raw-to-virtual latency remain
unqualified. User-operated simulator/hardware acceptance remains #199, verified
first in the progress lane. Overall estimate remains approximately **70% left**.
