# Installed aircraft catalogue notifications

The configured DCS-BIOS `AircraftAliases.json` and the selected X-Plane
`Aircraft` tree have their own catalogue baselines. Native notifications
refresh those baselines even while the simulator is closed. The game context
worker no longer uses the 30-second age threshold on process wakes. Process
and capture ownership stay separate from catalogue publication.

X-Plane watches one recursive tree per selected installation and watches
its anchors for creation or replacement. The reader preserves the existing
depth, entry and result bounds. The list depends on names and topology.
DCS watches the exact alias file for content and replacement changes.
Registration failures withdraw installed rows for the affected scope, report
the failure, and schedule a registration retry. Recovery captures a fresh
baseline. The saved list of previously seen vehicles remains available.

## Running Windows Cockpit

The previous native app missed the first nested X-Plane aircraft addition
within the 15-second fixture window. The rebuilt native WebView and
browser-dev each observed the empty baseline, two nested additions, a rename,
a removal, and the removal and restoration of the entire Aircraft tree through
the Games detail stream. Each client received seven updates after baseline. The fixture retained the
existing relative IDs and display names. Current vehicle stayed unset and
the status stayed waiting with the fixture executable closed. The fixture
read the retained detail stream during the file mutations.

Both clients showed an authored DCS alias appear and disappear while DCS was
closed. The fixture restored the original alias file and reported no cleanup
errors. The same runs passed the authored DCS-BIOS and ownship telemetry checks,
including the independent capture expiry and stalled consumer checks. Raw local
records are `.research/dcs-physics-native/1794110698718/evidence.json` for the
native WebView and `.research/dcs-physics-native/1794110786265/evidence.json` for
browser-dev.

Both rebuilt clients passed the existing Assetto watch outage, recovery and
nested runtime replacement fixture. The native executable SHA-256 was
`62adc3b9a4647e31253b7519b1aedd13f1ce02a2202946601948e6ac32aa961b`.
The browser-dev gateway SHA-256 was
`6be535485b715b389568b5f2d64c8b3d5cf56b875a48250a89c3412310311a1c`.

## Local checks

The full Windows native rerun passed 2,432 tests with 18 default ignores.
Both pinned DCS archive tests passed separately. The initial full run hit
Windows disk-full errors creating MCP test directories. It passed after
recovering workspace disk space and rerunning with the fixture apps closed.

All ten changed native production modules exceeded the repository's 71%
line threshold, with results from 88.79% to 100%. Declared source function
coverage ranged from 95.24% to 100%. LLVM's unfiltered function report for
`games.rs` was 66.67% and includes generated closures and test helpers.
The compiler did not instrument Rust branches. The timing audit accounts
for 453 current sites and all 247 baseline sites.

These fixtures use authored files and telemetry on Windows. Physical
latency, real simulators and Windows process event permission remain in
[#199](https://github.com/opensimgear/cockpit/issues/199). CI remains deferred
to the integrated qualification.
