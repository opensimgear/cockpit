# Cockpit for Windows x64 — development preview

Choose the `-setup.exe` installer, or extract the `-portable.zip` archive and run
`OpenSimGear-Cockpit/osg-cockpit.exe`. The portable app requires the Microsoft
Edge WebView2 Runtime; the installer can download WebView2 when needed.

The package includes the browser companion, licensing notices, and the pinned
optional vJoy and HidHide setup programs. Those driver installers run only when
explicitly selected in Cockpit; they are never installed automatically. Private
mapping workers run as modes of the Cockpit executable.

This preview is unsigned, so Windows may display a SmartScreen warning. It is
heavily under development. Existing live-output, provisioning, visibility and
script qualification gates remain in place. A downloadable package does not
establish physical hardware or game qualification.

ESP32 firmware images are not included in this release workflow. USB firmware
installation requires a Cockpit build with the corresponding embedded images.

`SHA256SUMS` covers the installer and portable archive. Each release is pinned to
the source revision shown by GitHub. The portable archive includes
`qualification.json` with executable and optional dependency provenance.
