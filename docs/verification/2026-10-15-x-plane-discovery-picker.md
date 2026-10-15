# X-Plane discovery and native executable picker correction

The installed Steam X-Plane 12 catalogue begins with
`2 1210 Wed Jun 24 15:46:43 2026`. The initial implementation incorrectly treated
that four-digit SDK revision as the six-digit runtime telemetry version. Discovery
now reads the catalogue's actual format; telemetry retains its separate runtime
version decoder. Both X-Plane tiles now use flight artwork, X-Plane wordmarks and
`X-Plane.exe` selection hints instead of falling through to the ACR presentation.

The game executable field is read-only with a native **Browse…** picker. The
desktop command selects one executable, uses the selected game's title and expected
filename, and seeds the directory from the current installation. Cancellation
leaves settings untouched. Only successful backend validation updates the selected
installation. A stale manual override can be cleared with **Use automatic
discovery**, including when the override points to a missing or invalid executable.

Validation on 2026-10-15:

- A Windows executable compiled from the production `game_discovery.rs` and
  `xplane_installation.rs` modules detected the actual Steam X-Plane 12 installation
  at `C:\Program Files (x86)\Steam\steamapps\common\X-Plane 12\X-Plane.exe`,
  with native launch available. X-Plane 11 was independently reported absent.
- Repeated Steam roots are deduplicated: the installed X-Plane 12 copy appears once.
- 330 native tests passed, with one existing ignored archive smoke test.
- 155 frontend transport/helper tests and four browser acceptance tests passed.
- Svelte checking reported zero errors/warnings; the production UI build passed.
- Windows native library type-check passed. The full GNU debug DLL cross-link
  exceeded the PE export-ordinal limit; native desktop linking with MSVC and
  manually exercising the OS dialog remain separate checks. The discovery probe
  was linked and run on Windows; browser picker tests use an authored IPC contract.

This verifies installation discovery, not live simulator telemetry or physical
hardware qualification. The simulator was not launched. The desktop application
must be rebuilt/restarted to load the new native dialog plugin and command.
