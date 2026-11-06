# Watched X-Plane profile scheduling

Supported X-Plane 11 and 12 profiles can now park on the retained native
telemetry journal and the existing host deadlines. The X-Plane owner already
commits component expiry to that journal and handles its own subscription
renewals. Process proof refresh and expiry remain separate host deadlines.

The gate requires an acquired X-Plane reader, live watch, validation worker
and the exact prepared game. Unsupported adapters, deferred inventory,
recovery or loss of a watch retain bounded service. The host fixture publishes
through a real X-Plane journal and verifies the host wake and fallback after
reader retirement. The UDP expiry regression verifies that expiry wakes a
passive observer without advancing capture identity.

Active simulator acceptance and Windows process-monitor consent remain in
#199.

## Verification

- All 2,356 native tests passed with 18 existing ignores. Host scheduling and
  source wakes reached 77.55% and 76.83% line coverage.
- All 14 telemetry frontend tests passed, and the audit verified 247 sites.
- The Windows dev build and three running Cockpit checks passed. They covered
  passive telemetry, the workspace baseline and controller preview catch-up.
- Dev executable SHA-256 was
  `29cd87fecce01ab937b00a1bf6120e67ab6d057f33674b1762a819de98137423`.
