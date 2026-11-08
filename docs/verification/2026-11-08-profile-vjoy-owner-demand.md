# External vJoy ownership in the profile slot picker

The virtual controller slot picker now requests the same shared native status
reader used by Devices and Settings Devices. Its existing visible lifetime
owns the demand. The reader stops when the document hides or the picker
closes. Ownership changes still use the existing `system.inventory`
invalidation and cached inventory response. This change adds no timer,
native sampler or routing work.

## Running Windows Cockpit

The previous frontend still showed Slot 1 as free after the external
feeder acquired it. The updated native WebView and browser-dev each
displayed busy on acquisition and free on release. Each then hid the
picker while the slot was busy, released the feeder while hidden, and
reopened on free. Both runs opened and closed exactly two demand
subscriptions, made zero hidden inventory reads, and made no explicit
inventory refresh request. The existing pin and Cockpit-owned vJoy
release regression also passed in both clients. Its preflight now waits
for the initial compatible inventory instead of treating a startup
snapshot as final.

Both clients used private databases and an existing free slot. The fixture
started a separate feeder and released it without sending an input report or
provisioning a slot. It counted cached reads for the picker and the mounted
Devices inventory separately from explicit refresh requests.

## Local checks

All 1,708 frontend tests, the Svelte check and production frontend build
passed. The slot projection's line, branch and function coverage each
reached 100%. The live fixture exercised the added demand and its
retirement. The app and gateway binaries were the same Windows builds
used for the [idle matrix](2026-11-08-idle-transport-matrix.md).
