# macOS and Linux auto-start file observations

The desktop auto-start owner now shares one retained `settings.autostart`
projection across Windows, Linux and macOS. The Linux watcher follows
`~/.config/autostart/<package name>.desktop`. The macOS watcher follows
`~/Library/LaunchAgents/<package name>.plist`. These match the pinned
auto-start plugin's Linux XDG and macOS LaunchAgent registration checks.

The watcher arms native notifications on existing ancestors before reading
status. It follows missing directories, parent replacement, and logical and
physical paths through symlinks. Relevant events and notification loss rearm
the source. While quiet, the worker parks until a source event or shutdown.
The existing General Settings reader and MCP resource use the same snapshot.

Under Ubuntu in WSL, the full Cockpit crate built with the repository lockfile
and passed all six auto-start tests. The locked file-watcher harness separately
passed four tests against real inotify. It covered nested directory creation,
rename and recreation, symlink referent creation/removal and retargeting,
unrelated and access-event filtering, and notification loss. LLVM measured
90.22% line and 92.86% function coverage for the Linux file watcher. A broader
Linux suite attempt could not reuse the memory-backed test executable after
the WSL session ended. The Apple Silicon file-watcher compile check passed with
the FSEvents backend. macOS runtime delivery still needs qualification on a Mac.

The rebuilt Windows dev Cockpit passed the registry and General Settings
lifetime fixtures. The same Run and StartupApproved changes reached the
portal and a read-scoped MCP client. The native executable SHA-256 was
`9c6cddb38f7ee5717fd2eac2ec34943270646f9a28dffc3dceec8af49eee65a4`.

The full Windows suite passed 2,419 tests with 18 default ignores. Both pinned
DCS archive tests passed separately. Production line coverage was 87.65% for
the file watcher, 86.11% for the registry watcher, 94.55% for the shared owner,
and 100% for MCP URI admission. LLVM module function coverage was 92.31%,
95.45%, 71.43%, and 100%, respectively. This instrumentation emitted no branch
data.
