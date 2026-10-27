param(
    [Parameter(Mandatory = $true)]
    [string]$Directory
)

$ErrorActionPreference = 'Stop'
$captureDirectory = (Resolve-Path -LiteralPath $Directory).Path
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Rdr2CaptureHotkey {
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
}
'@

# File requests are consumed by the opt-in native probe. Never inject game input or change focus.
$wasDown = $false
$logPath = Join-Path $captureDirectory 'hotkey.log'
Add-Content -LiteralPath $logPath -Value "$(Get-Date -Format o) F10 capture helper started"
while ($game = Get-Process -Name RDR2 -ErrorAction SilentlyContinue) {
    $down = ([Rdr2CaptureHotkey]::GetAsyncKeyState(0x79) -band 0x8000) -ne 0
    if ($down -and -not $wasDown) {
        [uint32]$foregroundProcess = 0
        [void][Rdr2CaptureHotkey]::GetWindowThreadProcessId(
            [Rdr2CaptureHotkey]::GetForegroundWindow(), [ref]$foregroundProcess)
        if ($game.Id -contains $foregroundProcess) {
            Set-Content -LiteralPath (Join-Path $captureDirectory 'native.request') -Value 'F10' -Encoding ascii
            Add-Content -LiteralPath $logPath -Value "$(Get-Date -Format o) F10 capture requested"
        }
    }
    $wasDown = $down
    Start-Sleep -Milliseconds 50
}
Add-Content -LiteralPath $logPath -Value "$(Get-Date -Format o) RDR2 closed; helper stopped"
