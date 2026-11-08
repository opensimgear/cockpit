# Companion listener settings after document hide

Settings → Companions subscribes to the listener while the panel and document
are visible. Hiding the document or leaving the panel retires the reader and
invalidates pending status replies. Reveal obtains a fresh baseline and one
listener-only status read before enabling controls. The form retains unsaved
inputs. Visible window focus can refresh the selectable network interfaces
after the observation is ready.

The previous frontend reproduced an active listener reader after document
hide in the running native Cockpit. The updated reactive navigation fixture
checks exact retirement, zero hidden listener-only reads, a different reader
with one baseline on reveal, and retirement when leaving the tab. Both native
WebView and browser-dev passed. Each final run used a private database and
enabled a listener on an ephemeral loopback port while the document was
hidden. On reveal the panel displayed the committed setting. The fixture
disabled the private listener during cleanup.

The local frontend suite passed all 1,705 tests. Svelte reported zero
diagnostics and the production frontend build passed. The shared visibility
helper reached 100% line, branch and function coverage. The running fixture
used the native and browser gateway builds verified in the
[setup watch recovery run](2026-11-08-setup-watch-recovery.md).
