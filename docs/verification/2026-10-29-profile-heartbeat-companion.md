# Profile heartbeat and companion feedback verification

Tested on Windows on 2026-10-29 with the running native dev Cockpit,
the existing paired Brave companion, and the actual DCS AH-6J cockpit.

## Failure and correction

The virtual output watchdog retains its 250 ms lease. Slow catalogue,
process, inventory, and panel work must not renew an old mapped decision.
Activation now renews only prepared neutral outputs using a bounded scoped
pump, which joins before arming and stops on cancellation or deadline.

Live game validation, inventory publication, and companion feedback have
bounded owned workers. A pending or expired proof withdraws the affected
game branch until a fresh baseline returns; physical input branches remain
independent. Current-reader checks, cancellation, and admission-time
predecessor capture prevent outgoing or returning readers from overwriting
a newer owner. Simulator commands still validate their exact authority.

A lost reader may reacquire the same unchanged verified game proof, using a
new reader UUID and advancing native baseline. A healthy reader cannot be
duplicated. Resume work carries the exact predecessor reader UUID so an old
worker cannot replace a later reader that happens to share the same proof.

Companion assignment checks also repeatedly parsed the entire compiled panel
manifest under the Hub mutex. Live timings showed one input polling cycle
taking 227 ms before reduction. Validated publications now retain the
immutable target-format fact, including after SQLite reload. Receipt,
installation, credential, and browser-only target checks remain enforced.
Native mapping and wire protocol generation counters are fenced separately.

An inventory mutex being busy no longer counts as a lost connection.
Established resource bindings retain their last committed validation result;
state mutations publish confirmed revocations before releasing the state lock.
The companion Hub similarly retains exact-receipt assignment confirmation
until the owning record changes. Native wire liveness and generations are
still checked on every poll. An unconfirmed binding pauses input admission
without clearing ready display feedback, and resumes from a neutral baseline.

An expired vehicle observation also no longer selects the Base variant or
starts a takeover. The applied variant is retained until a different vehicle
is positively observed. A confirmed unconfigured vehicle still selects Base;
an already paused vehicle transition stays paused until its target is known.

## Actual cockpit acceptance

`tests/ui/profile-activation-heartbeat-native.mjs` activated the saved
`DCS Feature Lab` revision 8 and observed the same owner for 300 seconds:

- 1,388 native status reads; no stop, recovery, fault, or owner replacement.
- 156 timed game reads; DCS was detected and its process verified throughout.
- Final state: Running, no degradation, no fault.
- Browser companion connected, panel assigned and armed.
- 464 feedback inspections; all four inputs received known native values,
  with zero unavailable samples after the five-second warmup.
- Final altitude: 159.7152 m; altitude warning: false; rudder: 50.01221%;
  aircraft panel brightness: 0%.

The soak overlapped the instrumented native suite. Some status reads were
temporarily Degraded while game evidence was pending (66 sampled reads). Those
branches recovered automatically without stopping the profile. The final
inspection had known values on every companion input. The browser panel was
also visually checked: its altitude and rudder bars and warning displayed data.

`tests/ui/profile-companion-ports-native.mjs` passed against the running
profile without opening Inspect. The selected block has separate Inputs and
Outputs headings, filters, compact port tiles, short type labels, and icon
actions matching the other blocks. This UI change was pushed separately as
`49d27e4`.

Live evidence is saved locally in `.research/activation-heartbeat-native/evidence.json`
and `.research/companion-port-layout-native/inputs-outputs.png`.

## Regression checks

- Companion service: 189 passed; companion serial: 38 passed.
- Portable mapping host: 123 passed; browser companion: 64 passed.
- Coverage runner unit tests: 6 passed.
- Native suite: 1,633 passed, zero failed, 11 ignored.
- Profile UI suite: 860 cases. Two timing assertions passed on an isolated
  rerun of the 35-case diagram suite after the parallel run.
- Svelte check on the atomic task snapshot: zero errors and warnings.
- Both actual native UI acceptance tests passed.
- All measured changed modules passed their 71% line coverage gates.

Native changed-module coverage ranged from 73.12% to 100%. The activation
heartbeat reached 98.03%, companion feedback 98.21%, pending companion
recovery 84.92%, and committed resource validation 100%. Companion service
feedback reached 100% and assignment confirmation 91.89%; native companion
input handling reached 95.69% in the portable mapping host.

Coverage artifacts are retained locally in
`.research/activation-heartbeat-final-coverage/` and
`.research/activation-companion-service-coverage/`.
