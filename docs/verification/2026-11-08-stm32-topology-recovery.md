# STM32 reconnect waits on shared serial topology

The STM32 gateway's manual Apply and automatic USB recovery now use the
workspace's existing serial topology notifications. When the exact gateway
is missing and coverage is live, the worker waits for a topology commit,
coverage loss, shutdown, or the original reconnect deadline. It rechecks
USB metadata before opening and verifies the gateway UID on the wire.
Automatic recovery also checks the expected controller lifecycle.

The existing bounded retry still applies when a matching port cannot open
or respond, or when notification coverage is unavailable. Manual Apply
preserves the hardware settle period and sends the write once. The shared
wait implementation serves physical CAN gateways and direct USB recovery.

The automatic fixture saw one initial enumeration and no additional scan
during an 80 ms quiet window. The arrival then caused one live revalidation
and reopened the same gateway. Another fixture injected a failed open on a
matching port and recovered without a second topology event. The manual
fixture re-enumerated the gateway at a different path after Apply, confirmed
the requested layout, and counted one Apply write.

The rebuilt Windows Cockpit passed the native workspace observation fixture
and both direct USB view fixtures in a private database. The workspace trace
showed no frontend discovery or state polling. These fixtures exercised the
running app and observation delivery. The mock gateway tests exercised the
reconnect and readback paths. Physical re-enumeration remains in #199.
Firmware layout completion and action-route readiness still have their
bounded retries.

The Windows suite passed 2,450 tests with 18 default ignores. Two ignored
DCS archive tests passed separately with the pinned archive. Ten focused
reconnect and serial inventory checks passed on the final fixture. All eight
changed production modules passed the 71% line coverage gate. The STM32
reconnect module reached 93.75% and the shared topology module reached 100%.

Dev executable SHA-256
`9cddfa3cec0f139e95e6ad3ebc6f5b3bf0ed0847b2b9ec77d4a5df9539a69819`.
