# Unlocked native qualification

The user confirmed that Windows was unlocked and requested another controlled
focus test. The actual Cockpit executable is main 229ec843, SHA-256
`5a1692faa0c27279f63a52f54e37982f06cff6a9919b41538157154aa9d007f3`.
No production code changes are required for these three previously blocked checks.

## Passed checks

- `desktop-focus-notifications-native.mjs`: the dedicated receiver observes the
  authored held key. Switching to the second receiver releases it and leaves
  parent and helper scope evidence ineligible, with no held controls. The helper
  epoch advances from 1 to 2. The paused registration survives 350 ms. Returning
  keeps epoch 2 and requires fresh neutral before a second key-down is accepted.
  Stop leaves held/possible-held empty, cleanup complete and uncertainty false.
- `controller-latest-state-native.mjs`: a slow preview retains one outstanding
  receipt while an independent preview advances from one to eleven updates.
  Native source sequence advances from 9382 to 9393. Releasing the receipt yields
  a complete 137-control snapshot at sequence 9393, with the same generation
  and no observer errors. This proves current-state recovery rather than stale
  replay; it does not measure physical motion-to-display latency.
- `vjoy-raw-output-native.mjs`: four authored debug transitions reach exact vJoy
  slot 1 as true/false/true/false, on one matching Windows Raw Input device.
  All four decoder statuses are `0x00110000`. Profile Stop releases the slot.
  This is a software source-to-destination check, not physical controller timing.

The receiver SHA-256 is
`b43f4ca2a98eaa2a21f880399164b8ea634dafe377fececcd4d729516aedfc1e`;
the Raw Input probe SHA-256 is
`23dc77ea38b8dc0a0b96d7d751e05021f6c3dc00b18e3df356192aa22a5d5051`.
Private traces remain under `.research/reactive`; device identifiers are not
needed for the public qualification summary.

## Latency fixture repair and remaining limit

The original two-source latency workload had obsolete setup assumptions. It now
uses the supported compiled flight publication and validates its physical button
and axis bindings. The original signed sinusoidal pointer workload is encoded
through the supported unipolar wire domain and explicitly normalized from native
0..32767 counts. Wire neutral 0.5 maps exactly to logical zero. Neutral is sent
before and after Apply; phases retain their original rates, durations, delivery,
ownership and audit assertions.

Each phase now captures its receiver through the gateway, saves an explicit
application reference with an expected revision, and checks the applied journal's
exact PID/executable/creation identity. Settings restoration uses the existing
configuration and selection APIs; the fixture no longer reads or rewrites legacy
JSON preferences. Failure diagnostics include actual scope receipts and current
foreground identity. Partial failures retain observed injection evidence rather
than interpreting incomplete qualification as absence of native input.
The interrupted traces precede that completion-flag correction; their receiver
events establish partial input independently of the old `injected` flag.

All 20 portable audit/workload regressions pass, including publication integrity,
480 waveform samples, identity rejection before profile writes, per-phase
revision guards and confirmed receiver input after partial failure. The Node
wrapper passes its syntax check. Production code and its preceding qualified
native build remain unchanged.

The sustained native workload **does not pass**. After successful setup and
warmup, a newly created `C:\Program Files\WSL\msrdc.exe` window takes foreground
focus during the first idle phase. One trace places its creation approximately
198 ms after the measurement marker. Both native scope watchers correctly revoke
delivery, release holds and require a fresh baseline. The same condition recurs
without concurrent desktop commands. Its launch origin was not established;
read-only investigation found no WSL launch in the exercised workload path.
The fixture does not force focus back, broaden scope or suppress the assertion.
Neither sustained idle/UI-load latency nor first-dispatch percentiles are
qualified by these interrupted runs. They remain software qualification under
#193/#198; physical preview/raw-to-virtual and simulator effect checks remain #199.

## Final cleanup

The wrapper reports no cleanup errors, no owned active profile, clean native
ownership, and terminated receive-only helpers. The externally acknowledged
parent audit prefix contains 22,429 records, with zero drops, evidence errors,
I/O errors or timeouts and queue high-water 10; it is not a completed latency run.
The final runtime is restored to audit-disabled mode, PID 39020 with the same
executable hash. Neutral scope replacement/Stop passes again; the profile is
stopped, held/possible-held lists are empty, uncertainty is false and all virtual
slots are free. CI remains deferred until feature implementation is complete.
