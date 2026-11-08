# macOS controller event probe

The opt-in probe retains IOHID matching and removal subscriptions before its
first inventory read. Per-device value callbacks publish bounded queue entries
and wake the capture worker. The worker reads only devices with ready values
or a baseline due. A removal retires the exact registry path before another
snapshot can admit it again. If removal identity cannot be recovered or the
bounded topology queue overflows, capture withdraws its inventory.
[Apple IOHIDManager callbacks](https://developer.apple.com/documentation/iokit/iohidmanager_h?changes=_4)

The old 4 ms input read and 500 ms inventory check are gone. A pending
suppression baseline can supply a deadline. When it expires, the worker
releases suppression and schedules a fresh baseline before parking.
The portable worker fixture verifies ready-value delivery, replacement of
the same path, deadline recovery, and idle shutdown. A separate fixture
verifies bounded retention of exact removals and failure on lost identity.

Local controller suites passed 175 tests on Windows and 180 on Ubuntu WSL.
The portable worker measured 98.06% LLVM line coverage and 100% function
coverage in the Windows test build.
Rebuilt Windows native and browser-dev Cockpit both passed the controller
inventory commit, restoration and reopen fixture. The controller crate,
examples and test targets type-checked for `aarch64-apple-darwin`. The
dependencies' documentation mode skipped native Swift and framework linking
during the cross-check. Native linking and physical IOHID delivery still need
macOS qualification in #199. Ordinary macOS capture stays behind its existing
platform gate.
