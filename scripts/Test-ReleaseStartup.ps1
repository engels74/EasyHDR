# Runs the actual release executable, including native dependencies and tray initialization.
# Requires a Windows desktop session with no other EasyHDR instance running.
# Artifacts are retained in a new temporary directory; the user's APPDATA is never used.
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $Executable,
    [string] $PreviousExecutable,
    [ValidateRange(5, 120)]
    [int] $TimeoutSeconds = 30
)

$ErrorActionPreference = 'Stop'
# Process.MainWindowHandle can select the off-screen HDR monitor window instead of Slint.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class EasyHdrStartupWindow {
    private delegate bool EnumWindowsProc(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr data);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    public static IntPtr Find(uint pid) {
        IntPtr result = IntPtr.Zero;
        EnumWindows((window, data) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            var title = new StringBuilder(512);
            GetWindowText(window, title, title.Capacity);
            if (owner == pid && IsWindowVisible(window) && title.ToString() == "EasyHDR") {
                result = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return result;
    }
}
'@
$candidate = (Resolve-Path -LiteralPath $Executable).Path
$launches = @($candidate, $candidate)
if ($PreviousExecutable) {
    $launches = @((Resolve-Path -LiteralPath $PreviousExecutable).Path) + $launches
}
$artifacts = Join-Path ([IO.Path]::GetTempPath()) ('easyhdr-startup-' + [guid]::NewGuid())
$appDirectory = Join-Path $artifacts 'EasyHDR'
New-Item -ItemType Directory -Path $appDirectory | Out-Null
Write-Host "Startup test artifacts: $artifacts"

# Keep the window visible so WM_CLOSE exercises the application's normal exit handler.
# Empty monitored_apps prevents the smoke test from toggling HDR for the user's applications.
@{
    monitored_apps = @()
    preferences = @{
        auto_start = $false
        monitoring_interval_ms = 1000
        show_tray_notifications = $false
        show_update_notifications = $false
        auto_open_release_page = $false
        minimize_to_tray_on_minimize = $false
        minimize_to_tray_on_close = $false
        start_minimized_to_tray = $false
    }
    window_state = @{ x = 100; y = 100; width = 660; height = 660 }
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $appDirectory 'config.json') -Encoding utf8

$iteration = 0
foreach ($binary in $launches) {
    $iteration++
    $process = $null
    $log = Join-Path $appDirectory 'app.log'
    # Do not allow a previous launch's readiness messages to satisfy this launch.
    if (Test-Path -LiteralPath $log) {
        Move-Item -LiteralPath $log -Destination (Join-Path $artifacts "startup-$($iteration - 1).log")
    }
    try {
        $start = [Diagnostics.ProcessStartInfo]::new($binary)
        $start.UseShellExecute = $false
        $start.WorkingDirectory = Split-Path -Parent $binary
        $start.EnvironmentVariables['APPDATA'] = $artifacts
        $start.EnvironmentVariables['RUST_LOG'] = 'info'
        $process = [Diagnostics.Process]::Start($start)
        $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        $ready = $false
        do {
            if ($process.HasExited) {
                throw "Launch $iteration exited before readiness: $($process.ExitCode) ($binary)"
            }
            $process.Refresh()
            $window = [EasyHdrStartupWindow]::Find($process.Id)
            $contents = if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Raw } else { '' }
            $ready = $contents -match 'System tray icon created successfully' -and
                $contents -match 'Running Slint event loop' -and $window -ne [IntPtr]::Zero
            if (!$ready) { Start-Sleep -Milliseconds 100 }
        } until ($ready -or [DateTime]::UtcNow -ge $deadline)
        if (!$ready) { throw "Launch $iteration did not reach tray, visible window and event-loop readiness." }
        if ($process.WaitForExit(3000)) { throw "Launch $iteration crashed after readiness: $($process.ExitCode)" }
        if (![EasyHdrStartupWindow]::PostMessageW($window, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)) {
            throw "Launch $iteration did not accept a window close request."
        }
        if (!$process.WaitForExit($TimeoutSeconds * 1000)) { throw "Launch $iteration did not exit after closing." }
        if ($process.ExitCode -ne 0) { throw "Launch $iteration failed during exit: $($process.ExitCode)" }
        Write-Host "PASS launch $iteration : tray, window, event loop, sustained startup and normal exit ($binary)"
    } finally {
        if ($process) {
            if (!$process.HasExited) { $process.Kill(); $process.WaitForExit() }
            $process.Dispose()
        }
        if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Tail 8 | Write-Host }
    }
}
