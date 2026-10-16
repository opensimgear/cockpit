# Desktop preferences

Cockpit shows its application icon in the system tray (menu bar on macOS).
The tray menu offers **Open Cockpit** and **Quit**. Clicking the icon also opens
the window on Windows and macOS; on Linux, use the menu. Launching Cockpit again
restores the existing window instead of opening another copy.

In **Preferences → Desktop behavior**:

- **Close to tray** hides the window when its close button is pressed.
- **Minimize to tray** hides the window when minimized.
- **Start Cockpit when I sign in** enables or disables login startup on Windows,
  macOS, and Linux, using Tauri's native autostart plugin. It starts the normal
  Cockpit window at sign-in. The setting reflects the operating system's current
  registration rather than a separate saved flag.

Both tray preferences default to off and are saved in
`desktop-preferences.json` in the application's configuration directory.
Hiding the window keeps gateway connections, mappings, and integrations running.
Use **Quit** in the tray menu to shut down Cockpit and release its connections.

Linux requires a desktop with system tray/AppIndicator support. If creating the
tray fails, the window keeps its normal close/minimize behavior and the tray
settings are disabled. A desktop without a visible tray host may still accept
an AppIndicator; enable tray behavior only if the icon is visible.

The browser preview displays a desktop-only message instead of these controls.
Preference read/write errors and auto-start errors are displayed in Preferences;
failed saves do not change the active tray behavior.
