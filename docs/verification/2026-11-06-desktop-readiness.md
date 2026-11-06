# Live desktop output readiness

The native host derives a compact desktop readiness state from the same
conditions used by the explicit desktop diagnostic snapshot. A separate
observer captures transitions even when the generic profile session status
is unchanged. Encoding and publication use the existing profile read-model
worker. Desktop changes do not republish the generic profile runtime topic.

The `profiles.desktop` topic carries stopped, active, paused, waiting, faulted
or recovery-needed state. It excludes application identities, helper scope
and input or journal history. Debug subscribes while Runtime & recovery is
visible and releases the subscription on exit. Detailed diagnostics remain an
explicit read.

The running Cockpit verified the idle baseline in Debug, the native snapshot,
and MCP resource discovery. The native transition fixture verifies delivery
to both UI and MCP, including waiting, active, paused and recovery-needed.
Physical focus changes and desktop target timing remain tracked in #199.

## Verification

- Native suite passed with 2,355 tests and 18 existing ignores. The profile
  read-model worker and its bound accounting covered 96.94% and 100% of
  production lines.
- All 69 mapping-host desktop tests and 10 frontend read-model tests passed.
  Svelte diagnostics found zero errors or warnings. The broader routing UI
  harness failed on its missing profile activation mock. The same failure
  reproduced with the prior Debug component.
- The Windows dev build and running MCP/portal check passed. Dev executable
  SHA-256 was
  `a25793426448bcb1c4bc83fb506998d1b262a9b58fb6d5268a083599d8462893`.
