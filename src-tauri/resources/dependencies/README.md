# Optional Windows dependencies

These unmodified upstream installers are bundled for explicit setup from Cockpit
on Windows x64. They are never run at startup. Their versions match the existing
vJoy and HidHide adapters, rather than following upstream `latest` releases.

`manifest.json` records the official download URLs, exact byte sizes and SHA-256
digests. Both downloaded installer Authenticode signatures validated on
15 October 2026: vJoy is signed by **Brunner Elektronik AG**, HidHide by
**Nefarius Software Solutions e.U.**. Cockpit checks size, digest and a valid
Authenticode signature again before launching the interactive installer.

The complete upstream MIT notices are preserved in `vJoy-LICENSE.txt` and
`HidHide-LICENSE.txt`; HidHide's installer license is also preserved verbatim in
`HidHide-LICENSE.rtf`. The installers retain their embedded notices and license
dialogs. Upstream source for the corresponding releases:

- https://github.com/BrunnerInnovation/vJoy/tree/v2.2.2.0
- https://github.com/nefarius/HidHide/tree/v1.5.230.0

Tauri copies this directory to `dependencies/` in the application resources.
The standalone Windows staging script copies the same directory alongside the
executable. Do not rename an installer without updating the catalog. When
updating, review the matching driver/DLL pins in the platform adapters, download
from the official release, validate the publisher signature, preserve licenses,
update size/digest, and run the `optional_dependencies` native tests.

Setup uses the upstream wizard and normal Windows administrator authorization.
It accepts no caller-supplied path, URL or installer arguments. Only one setup
can run at a time. Cancellation, installer failures and restart exit codes are
reported; successful setup is not proof of driver readiness. Complete any restart
and use **Inspect vJoy** or **Inspect HidHide**. Existing mapping/visibility
qualification gates and opt-ins are independent of installation.

No installer was executed to verify this integration. Interactive UAC,
cancellation, upgrade and restart behavior still need a native desktop check.
