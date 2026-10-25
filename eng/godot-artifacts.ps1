# Shared artifact paths and copies for the Godot build and validation scripts.
function Copy-RsfGodotArtifact([string]$Source, [string]$Destination) {
    $taskSourceHash = (Get-FileHash -LiteralPath $Source -Algorithm SHA256).Hash
    if ((Test-Path -LiteralPath $Destination) -and (Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash -eq $taskSourceHash) { return }
    [IO.Directory]::CreateDirectory((Split-Path -Parent $Destination)) | Out-Null
    $taskTemporary = $Destination + '.' + [Guid]::NewGuid().ToString('N') + '.tmp'
    $taskBytes = [IO.File]::ReadAllBytes($Source)
    if ([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($taskBytes)) -ne $taskSourceHash) {
        throw 'Godot artifact source changed or could not be read consistently.'
    }
    $taskOutput = [IO.FileStream]::new($taskTemporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write,
        [IO.FileShare]::None, 1048576, [IO.FileOptions]::WriteThrough)
    try {
        $taskOutput.Write($taskBytes, 0, $taskBytes.Length)
        $taskOutput.Flush($true)
    } finally { $taskOutput.Dispose() }
    if ((Get-FileHash -LiteralPath $taskTemporary -Algorithm SHA256).Hash -ne $taskSourceHash) { throw 'Godot artifact copy failed its hash check.' }
    [IO.File]::Move($taskTemporary, $Destination, $true)
}

function Copy-RsfGodotAddon([string]$Source, [string]$Destination) {
    foreach ($taskFile in Get-ChildItem -LiteralPath $Source -Recurse -File) {
        $taskRelative = [IO.Path]::GetRelativePath($Source, $taskFile.FullName)
        Copy-RsfGodotArtifact $taskFile.FullName (Join-Path $Destination $taskRelative)
    }
}

function Get-RsfGodotBuild([string]$Root, [string]$Edition, [string]$Target = 'editor') {
    $taskCategory = if ($Target -eq 'editor') { 'engines' } else { 'templates' }
    $taskManifest = Join-Path $Root ".local/godot/$taskCategory/$Edition/$Target.json"
    if (!(Test-Path -LiteralPath $taskManifest)) { throw "Build the matching Godot $Edition / $Target artifact first." }
    $taskBuild = Get-Content -Raw -LiteralPath $taskManifest | ConvertFrom-Json
    if (!(Test-Path -LiteralPath $taskBuild.executable) -or (Get-FileHash -LiteralPath $taskBuild.executable -Algorithm SHA256).Hash -ne $taskBuild.sha256) {
        throw "Godot build artifact does not match its manifest: $taskManifest"
    }
    return $taskBuild.executable
}

function Set-RsfGodotForeground([Diagnostics.Process]$taskProcess) {
        if (!('GodotValidationFocus' -as [type])) {
            Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class GodotValidationFocus {
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, IntPtr process);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint from, uint to, bool attach);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr window);
    public static bool Activate(IntPtr window) {
        uint current = GetCurrentThreadId();
        uint foreground = GetWindowThreadProcessId(GetForegroundWindow(), IntPtr.Zero);
        bool attached = foreground != 0 && foreground != current && AttachThreadInput(current, foreground, true);
        try { BringWindowToTop(window); SetForegroundWindow(window); return GetForegroundWindow() == window; }
        finally { if (attached) AttachThreadInput(current, foreground, false); }
    }
}
'@
        }
        $taskFocused = $false
        for ($taskAttempt = 0; $taskAttempt -lt 50 -and !$taskProcess.HasExited; ++$taskAttempt) {
            $taskProcess.Refresh()
            if ($taskProcess.MainWindowHandle -ne [IntPtr]::Zero -and [GodotValidationFocus]::Activate($taskProcess.MainWindowHandle)) {
                $taskFocused = $true
                break
            }
            Start-Sleep -Milliseconds 100
        }
        if (!$taskFocused) {
            throw "Cannot establish the actual foreground window required by the DLSS-G fixture: $($taskProcess.Id)"
        }
}
