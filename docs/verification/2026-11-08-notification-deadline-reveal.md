# Notification deadlines on reveal

The Windows dev desktop and Edge browser-dev each saved a temporary profile
using the real editor and gateway with a private database. The success
notification appeared once and stayed current across navigation from
Profiles to Overview.

The fixture hid the document and delayed only the notification's dismissal
callback to reproduce a throttled renderer. The previous build left the
expired notification visible after reveal on both transports. The corrected
application clears it on the visibility event, before the delayed callback
can run. Returning to the editor did not replay the notification. Native
readback confirmed the new profile name and exactly one saved revision.

Reveal compares the existing deadlines once. Notifications with time
remaining keep their deadlines, and hover and keyboard focus still hold
the countdown. The store publishes only when an elapsed notice is removed.
Nine focused tests passed with 100% line and function coverage and 94.83%
branch coverage. Svelte checking reported zero errors or warnings.
All 1,715 frontend tests and the production frontend build passed.

Run [the fixture](../../tests/ui/notification-deadline-reveal.mjs) with
`OSG_NOTIFICATION_REVEAL_NATIVE=1` or
`OSG_NOTIFICATION_REVEAL_BROWSER_URL` against a private dev session.
