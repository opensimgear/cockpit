# Idle profile coordinator scheduling

The native coordinator parks without a service timer when it has no applied
owner or staged launch. Its idle poll did not service an adapter. Explicit
profile mutations already notify the worker after admission and ownership
changes, and shutdown notifies it before joining. Native foreground observer
termination has its own wake.

The scheduler retains the bounded launch interval and active fallback where
adapters still need service. The native regression parks with the idle
coordinator's real wait policy and observes Apply's committed ownership wake.

## Verification

- All 2,357 native tests passed with 18 existing ignores. Scheduling covered
  all 27 production lines.
- The Windows dev build and two running Cockpit tests passed. The output-free
  controller profile applied, shared capture with preview, survived replacement
  and stopped. The workspace subscription baseline also passed.
- The timing audit verified 247 sites. Dev executable SHA-256 was
  `67d08497c9c79ace05b4062dd7772ee6b400fc58c41877c57790e9b8a7b6012a`.
