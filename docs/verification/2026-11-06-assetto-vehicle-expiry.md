# Watched Assetto vehicle telemetry expiry

All four Assetto integrations share this vehicle reader path. Once the native
capture watch is installed, the vehicle worker reads it only when the watch
signals new state. The maintenance pass can still verify process, installation
and setup evidence without decoding the same packet again.

The original physics page timestamp is the oldest page in a qualified capture.
It anchors the decoder's freshness deadline. At expiry, the worker clears
live vehicle telemetry locally, without reading the source or renewing process
evidence. A capture that arrives in the same worker pass takes priority over the
expiry path. The next packet must still satisfy the decoder's normal continuity
and requalification rules.

The approved shared-memory acquisition owner retains its existing cadence. The
100 ms process verification pass and maintenance for readers without a capture
watch remain in #188. Profile delivery and the underlying telemetry source
transport are outside this vehicle worker change.

- The Windows dev build passed. Its executable SHA-256 is
  `221fe5d1ae9af11d29ba9a3421bd17b2418ffd5ab300832fe2bafe46d583385e`.
- The default native suite passed 2,342 tests with 17 ignored. The new regression
  counts two reads for two capture wakes, no read during process maintenance
  and no read at expiry. All five coverage gates passed: telemetry session
  94.68%, vehicle sources 87.15%, tracker 97.87%, polling 94.62%, and context
  runtime 95.46%.
- The running Cockpit kept a single telemetry inspector subscription through
  filtering and game changes and closed it on exit. Two authored, unlaunched
  entries supplied the otherwise empty dev catalogue and were removed after
  the check. All 14 frontend telemetry tests passed.
- The older expanded-telemetry fixture stopped at the retired `game_installation`
  operation. The current four-game capture fixture reached native setup, where
  Windows denied WMI process-event access (`0x80041003`). The authored publisher
  therefore did not qualify live telemetry in the running app. User consent for
  the process monitor and real simulator acceptance remain in #199.
