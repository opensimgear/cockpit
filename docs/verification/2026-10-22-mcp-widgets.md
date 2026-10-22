# MCP widget authoring

Verified on the running main Windows dev cockpit through the official
`@modelcontextprotocol/sdk` client and authenticated loopback MCP endpoint.

The shared native widget library supports `widgets_list`, `widget_get` and
write-scoped `widget_save`. The `cockpit://guide/widgets` resource provides
portable source examples and revision rules. Existing UI libraries merge into
native storage; saves preserve immutable revisions and pinned panel instances.
No panel publication, assignment or firmware operation is part of widget saving.

## Results

| Check | Result |
| --- | --- |
| Frontend/package suite | 1,259 passed |
| Broad instrumented native suite | 1,149 passed, 8 existing ignored tests |
| Final native widget regressions | 7 passed |
| Final MCP adapter/schema regressions | 17 passed |
| Final companion-service suite | 122 passed |
| Cockpit and browser Svelte checks | 0 errors, 0 warnings |
| Generated publication/library fixtures | 8 verified |
| New widget library production line coverage | 184/185, 99.46% |
| Added MCP dispatch/resource lines | 4/4 covered |
| Shared publication/source asset validation module | 647/813, 79.58% |

The broad native run preceded the final response-field adjustment and additional
asset/concurrency regressions; the final instrumented focused suites cover those
changes. The existing MCP adapter as a whole measured 63.23%; its added dispatch
and resource lines are fully covered. Its unrelated existing branches are not
claimed to meet a new whole-module coverage threshold.

`tests/ui/mcp-widgets-native.mjs` verifies actual tool discovery, source guide,
creation/revision, exact operation replay, stale and malformed rejection,
read-only credential denial, automatic appearance in Widgets, editor opening,
and UI-save-to-MCP readback. Temporary test credentials are revoked and the
previous MCP enabled/port settings restored. One demonstration widget remains
in the disposable dev library. No panel assignments or device outputs change.

Evidence is retained locally under `.research/dev/mcp-widget-*` and
`.research/mcp-widget-coverage/`. Native tests also verify restart persistence,
asset integrity and isolation, concurrent creation, failed-write rollback,
immutable imports and corrupted-storage rejection. Widget requests have their
own UI queue; a regression checks that slow inventory cannot delay them or
emergency profile Stop.
