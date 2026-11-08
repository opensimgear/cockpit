# Explicit launch uses the shared process observer

The explicit profile launch takes an initial exact process reading after
spawn. If the process has yet to appear, the request waits for a committed
process observation, a launch control notification or the original 45-second
startup deadline. Process identity and admission are checked again before a
game can be used. A spurious wake keeps the same deadline and does not
trigger another process read. The automatic Windows discovery preference and
its approved one-second fallback remain on the shared observer.

The regression supplies a wake without a producer notification and confirms
one exact process check, expiry and a single launch attempt. Existing tests
cover cancellation during the native process read, invalid process identity,
process replacement and uncertain spawn.

The full Windows native suite passed 2,459 tests with 18 default ignores.
All 28 focused Windows native tests and five frontend timer guards passed.
The launch module reached
96.31% production line coverage and 96.43% LLVM function coverage. The
instrumentation emitted no branch counts. The coverage trace shows two
parks and one deadline expiry for the regression's single process check.

The rebuilt Windows dev Cockpit launched an output-free executable fixture
from an idle session, verified its process and applied the default profile.
The fixture process, launch owner and temporary catalogue row were cleaned
up. The app ran against a private database. Executable SHA-256
`dcc8ccd4a78c47d4f86d0f0d28329d50843c39fefa727317c1519f0a6cb92546`.

The protocol-1 Pico retirement removed the only caller of the macOS HID
naming helper. Its remaining module, registry writes and 100 ms retry were
unreachable from the running app and have been removed. Current Pico capture
and inventory use their own protocol and native owners.
