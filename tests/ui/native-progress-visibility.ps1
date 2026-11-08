param(
    [Parameter(Mandatory)][int]$ProcessId,
    [Parameter(Mandatory)][ValidateSet('hide','reveal')][string]$Action
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class ProgressVisibilityFixture {
    private delegate bool WindowVisitor(IntPtr window, IntPtr context);
    [DllImport("user32.dll")] private static extern bool EnumWindows(WindowVisitor visit, IntPtr context);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out int processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr window, StringBuilder title, int capacity);
    [DllImport("user32.dll")] private static extern bool ShowWindowAsync(IntPtr window, int command);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);

    public static long SetVisibility(int processId, bool visible) {
        var matches = new List<IntPtr>();
        EnumWindows((window, context) => {
            int owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner == processId) {
                var title = new StringBuilder(128);
                GetWindowText(window, title, title.Capacity);
                if (title.ToString().StartsWith("Profile progress", StringComparison.Ordinal)) matches.Add(window);
            }
            return true;
        }, IntPtr.Zero);
        if (matches.Count != 1) throw new InvalidOperationException("Expected exactly one progress window owned by the fixture process.");
        var popup = matches[0];
        if (IsWindowVisible(popup) == visible) throw new InvalidOperationException("The progress window already has the requested visibility.");
        if (!ShowWindowAsync(popup, visible ? 4 : 0)) throw new InvalidOperationException("Could not change the progress window visibility.");
        var wait = Stopwatch.StartNew();
        while (wait.ElapsedMilliseconds < 3000 && IsWindowVisible(popup) != visible) Thread.Sleep(10);
        if (IsWindowVisible(popup) != visible) throw new InvalidOperationException("The progress window did not change visibility.");
        return popup.ToInt64();
    }
}
'@
[ProgressVisibilityFixture]::SetVisibility($ProcessId, $Action -eq 'reveal')
