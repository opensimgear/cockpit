# Device actions acceptance — 2026-10-13

Milestone 1 provides manual device-advertised actions through the normal Cockpit
Rust adapter and production gateway/device handlers. DCS-BIOS bindings and panel
input event delivery remain subsequent milestones.

The device workspace renders controls from bounded descriptors, without board
name matching: boolean, enum, signed integer, RGB, UTF-8 string and no-argument
actions. The current physical board's status LED advertises `auto`/`on`/`off`;
native JSON devices advertise an RGB LED and a boolean LED. Applied readback is
software state, rather than electrical measurement.

## Checks

- `pnpm check`: zero errors and warnings.
- `pnpm transport:test`: 53 passing frontend/bridge tests, including five new
  action helper scenarios.
- `pnpm ui:build`: production frontend build passed.
- Native Rust suite: 77 passing tests. Four existing socket tests require local
  loopback/socket access; the complete suite passed with that access enabled.
- Official Svelte autofixer: no issues or suggestions for `App.svelte`,
  `ManualActions.svelte` and `ActionControl.svelte`.
- Production process acceptance: `Cockpit discovers advertised LED actions and
  routes manual outputs independently of HID and writer ownership` passed using
  the normal `osg-gateway-service`, two independently running C gateway processes
  and two JSON device processes with the same module identity.

The process test covers typed discovery, RGB/boolean invocation and applied
readback, unchanged HID descriptors/configuration, no writer claim, invalid
arguments, stale catalogue/boot rejection, disabled HID, direct USB input
ownership, isolation between gateways, lost acknowledgement and reconnect/readback
recovery. Native tests additionally cover atomic pagination, changed revisions,
duplicate descriptors, explicit unsupported firmware, bounded route-read retries
and failed callback uncertainty. Invocations are never automatically replayed.

## Coverage

Measured production code coverage exceeds 70%, excluding Rust unit test bodies:

| Scope | Covered lines | Line coverage |
| --- | ---: | ---: |
| Rust action descriptor/value validation and encoding | 173 / 177 | 97.74% |
| Rust action route, transport and request methods | 180 / 195 | 92.31% |
| TypeScript action helper execution | — | 87.50% |

The TypeScript helper also has 97.06% branch and 100% function coverage. Its
`pnpm actions:coverage` command enforces a 71% minimum on all three measures.
The native `scripts/action-coverage.py` gate enforces more than 70% production
line coverage in CI. Rust coverage used the matching toolchain's `llvm-tools-preview`,
`RUSTFLAGS='-C instrument-coverage'`, a clean `LLVM_PROFILE_FILE` directory,
`llvm-profdata merge -sparse`, and `llvm-cov show` with line counts. Production
ranges stop before `#[cfg(test)]` in `actions.rs`, and cover `action_route`,
`action_command` and `action_request` in `stm32.rs`.

Research builds, profiles, process state and acceptance artifacts were kept under
the workspace `.research/` directory. No hardware LED, physical USB/CAN electrical
behavior or DCS integration is claimed by these software tests.

## Browser verification

The Cockpit web interface discovered a live native panel in the private
`actions-ui` namespace. Its generated controls applied RGB `12ABEF` and boolean
LED on, and both UI and device readback agreed. The settings stayed at generation
1 with no writer claim, flash writes, pending apply, descriptor change or module
reconnect. Screenshots and exact readback are retained in workspace
`.research/device-actions/ui-smoke/`.
