# Controller preview and applied profile capture sharing

The strengthened `tests/ui/profile-source-wakes-native.mjs` ran against the
Windows dev executable from `d56551c3` with SHA-256
`0be5a759a05f8f4475d7fb27bb6d4a92089bf57492f5dde82e47c0749a34b8dc`.
It waited for the native WebView before accessing the gateway. The isolated
fixture assigned an attached controller's observed integer port to an identity
node with an unbound output.

- Two preview listeners received the same immutable state through one native
  subscription. Preview alone left the profile stopped. The applied profile
  then reported the matching axis value from the same source. The previous
  [exact capture timestamp comparison](2026-11-05-controller-latest-preview.md#shared-capture-with-an-applied-profile)
  verifies the report's provenance separately from this run's liveness clock.
- Both listeners retired. The real native unsubscribe returned successfully,
  and the preview received no more frames. Over a 1,750 ms closed interval,
  the same profile owner kept a known routed value and its liveness timestamp
  advanced by 1,752,320 µs.
- Reopening acquired one new subscription with a current baseline, shared by
  both listeners. Two opens and two successful retirements were recorded,
  with zero controller inspection queries. Same-profile replacement acquired
  a new owner and its baseline. Stop cleared the owner and rejected stale
  inspection. The fixture deleted the temporary profile.

The running app reported zero USB module endpoints. The current native test
`usb_preview_and_live_profile_share_capture_without_owning_each_other`
passed against the real Fleet observation bridge and profile worker with an
authored USB report. It verifies the same capture clock and Instant in both
paths and a second report reaching the worker after preview unsubscribe.
The native HID dual-rate routing and overflow test also passed at 1,000.80
and 125.17 Hz of authored capture.

All 1,705 frontend and timer-guard tests passed. Stream transport coverage
was 98.28% lines, 93.06% branches and 92.86% functions. Controller
observations reached 98.68%, 96.74% and 97.37%. The production executable
was unchanged.

The live fixture used no virtual, desktop or simulator output. Real USB
module sharing in the running app, physical controller unplug and replug,
and raw-to-destination and preview latency remain in
[#199](https://github.com/opensimgear/cockpit/issues/199). The live record is
`.research/reactive/profile-source-wakes-native/1794079406836/evidence.json`.
