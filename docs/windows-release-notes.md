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

Production builds obtain the latest published `esp32-companion` firmware release
when you check for updates or install a board. Cockpit validates the selected
board, image layout and checksums before flashing. An internet connection and a
published firmware release are required. Development Cockpit builds instead use
locally staged firmware images.

`SHA256SUMS` covers the installer and portable archive. Each release is pinned to
the source revision shown by GitHub. The portable archive includes
`qualification.json` with executable and optional dependency provenance.
