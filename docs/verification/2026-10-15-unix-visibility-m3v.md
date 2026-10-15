# M3V: Linux/macOS visibility implementation and qualification gaps

Issue #29 remains open. Native visibility/suppression is disabled by default and by compiled qualification gates. Saving intent, inspecting capabilities and importing settings cannot grab, seize, launch or alter device nodes. Virtual output remains a separate opt-in.

## Implemented

The per-controller UI distinguishes Windows HidHide policy from Linux/macOS event suppression. Unsupported scopes are rejected by the service, not silently broadened. macOS per-process hiding is unavailable. Linux already-running processes and unsupported launch paths are rejected. Controlled launch has no approved isolation helper and fails before launching a child or changing namespaces; it is not advertised as an implemented native boundary.

Broad suppression is owned by the existing capture handle. A bounded command slot carries the exact source/session/generation/interface baseline to the capture thread. A new session validates identity before applying, records every attempted handle before mutation, and releases every owned handle on partial failure. Linux uses EVIOCGRAB through the existing evdev descriptor. macOS closes/reopens the existing IOHID device using seizure options and replaces callback subscriptions/queues. Both native entry points remain gated off.

Each transition invalidates captured values and requires a fresh kernel baseline within two seconds. Capture gaps, identity changes, disconnects and Stop release the session; failed release closes the owned capture handle. Capture thread exit/panic closes handles. Reconnect does not reapply suppression. Kernel handles supply ephemeral ownership, so no persistent hiding policy is replayed at startup. A stuck OS call still needs native failure qualification and is not claimed to have a proven recovery bound.

## Automated evidence

Portable fixtures exercise same-handle ownership, partial apply/release failures, stop-before-apply, stale identities, gaps, disconnects, baseline timeout, fresh baselines, unsupported policies and missing helper denial. Tests make no real grabs or seizures. Per-module coverage is required above 70% on all native CI platforms. Windows Tauri continues to run with unqualified actions disabled.

## Remaining acceptance

Real selected/unselected probe clients must establish event delivery versus enumeration and alternative hidraw/raw USB/HID paths. Linux isolated launch must still implement and qualify an owned namespace helper that denies selected physical evdev/js/hidraw nodes while exposing virtual devices and leaving host clients unaffected. macOS seizure needs a supported device/API/OS matrix, permissions and cleanup tests. Real Stop/crash/reconnect and visibility-only/output-only checks remain open. No fixture or cross-target type-check qualifies these native capabilities.
