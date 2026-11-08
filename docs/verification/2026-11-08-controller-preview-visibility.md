# Controller input preview visibility

The Devices controller detail and profile controller inspector share the
physical controller preview reader. Hiding the document releases that
reader and cancels its pending presentation frame. Other readers keep
their own demand. When the preview returns, its current baseline displays
immediately. While waiting for a baseline, the controller card retains its
identity and unsaved fields with input values cleared. Continuous visible
updates still coalesce to one presentation per dirty frame.

## Running Windows Cockpit

The previous frontend kept the exact controller input subscription alive
after the fixture drove the document-hide event. The updated native WebView
and browser-dev released the old reader and acknowledged a fresh baseline on
a new subscription when revealed. The production detail kept the same card
and unsaved draft. The reopened input observation contained the controller
name committed while hidden, and the displayed report counter caught up to
that observation. A second consumer shared the existing transport
subscription. Neither client called `controller_inspect` to render input.
The fixture used disposable databases and restored the controller name.
The neighboring Device visibility reader fixture also passed on both clients.
The native profile controller inspector fixture rendered all controls,
saved settings, and verified that the profile stayed inactive.

The running checks used the native and browser gateway binaries from
[aircraft catalogue verification](2026-11-08-vehicle-catalogue-wakes.md).
The visibility event and metadata change exercise subscription lifecycle.
The frame cancellation, continued independent demand and immediately
rendered replacement sample are covered by the controller stream tests.
Physical controller-to-preview and virtual-output latency remain in
[#199](https://github.com/opensimgear/cockpit/issues/199).

## Local checks

All 1,708 frontend tests passed. Both Svelte checks reported zero errors and
warnings, and the production frontend build passed. The controller
observation module measured 98.72% lines, 96.83% branches and 97.44%
functions. The shared visibility helper measured 100% on all three.
