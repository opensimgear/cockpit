# DCS Saved Games profile discovery

The discovery limit now applies to matching custom profiles. The scanner
retains the first 128 custom profiles in path order and keeps the standard
folders and the installation's explicit `dcs_variant.txt` target eligible.
The native watcher uses the same case-insensitive names as discovery. An
installation hint for a differently cased path to the same folder appears
once in the candidate list.

The native filesystem regressions use private roots. They cover discovery
and removal with 160 unrelated directories, rejection of matching regular
files, ordering with 130 custom profiles, and an explicit variant outside
the automatic limit. The Windows test checks a case-insensitive alias for
the same folder.

The Windows development executable had SHA-256
`0be5a759a05f8f4475d7fb27bb6d4a92089bf57492f5dde82e47c0749a34b8dc`.
The running Cockpit passed the DCS catalogue editor fixture in 7.50 seconds,
including watch re-registration and draft preservation. It also passed the
managed DCS setup and independent BIOS and ownship fixture in 72.81 seconds,
including setup repair, competing process withdrawal, pause and export
denial, respawn, and process exit. The fixture used a private installation
and reported no cleanup errors.

The high-entry-count scenarios ran against native temporary directories in
the test suite. The live fixtures exercised normal editor and setup paths.
Windows process-event consent and the process fallback disposition remain
open. Raw evidence is under `.research/reactive/dcs-discovery-cap/` and
`.research/dcs-physics-native/1794077578234/evidence.json`.

## Local tests and coverage

The complete Windows run passed 2,423 tests and had 18 default ignores.
One Assetto test needed a 512 MiB temporary file and passed on the same
binary after disk space was recovered. Both pinned DCS-BIOS archive checks
and 51 focused frontend tests passed separately. The separately recorded
setup, game context and integration coverage runs reached 86.78–98.53%
production lines and 71.43–85.71% LLVM functions across the changed modules.
Rust branch coverage was unavailable in this build.
