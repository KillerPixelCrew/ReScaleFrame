param(
    [Parameter(Mandatory)][ValidateSet('current','history','rectified')][string]$Stage,
    [switch]$Aged
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
. (Join-Path $taskRoot 'eng/godot-artifacts.ps1')
. (Join-Path $taskRoot 'eng/godot-process-isolation.ps1')
. (Join-Path $taskRoot 'eng/godot-capture-foreground.ps1')
$taskLease = Enter-RsfGodotValidation
try {
    $taskBase = Join-Path $taskRoot '.local/godot/visual/fsr2-jitter-sign-pp-ordinary-object'
    $taskDiagnostics = Join-Path $taskRoot '.local/godot/fsr2-stages-20261025'
    $taskCase = if ($Aged) { 'aged' } else { 'ordinary' }
    $taskProject = Join-Path $taskDiagnostics "$Stage-$taskCase"
    if (Test-Path -LiteralPath $taskProject) { throw 'Choose a fresh diagnostic output; existing evidence is retained.' }
    $taskFrames = Join-Path $taskProject 'frames'
    $taskAddon = Join-Path $taskProject 'addons/rescaleframe'
    New-Item -ItemType Directory $taskFrames,$taskAddon -Force | Out-Null
    foreach ($taskName in @('project.godot','main.tscn','main.gd','scene.gd')) {
        Copy-RsfGodotArtifact (Join-Path $taskBase $taskName) (Join-Path $taskProject $taskName)
    }
    $taskBaseAddon = Join-Path $taskBase 'addons/rescaleframe'
    foreach ($taskEntry in Get-ChildItem -LiteralPath $taskBaseAddon -Force) {
        if ($taskEntry.Name -in @('runtime','bin','runtime-manifest.json')) { continue }
        Copy-Item -LiteralPath $taskEntry.FullName -Destination (Join-Path $taskAddon $taskEntry.Name) -Recurse -Force
    }
    Copy-RsfGodotArtifact (Join-Path $taskRoot '.local/godot/fsr2-diagnostic/ReScaleFrame.Godot.Native.EE22.dll') (Join-Path $taskAddon 'bin/ReScaleFrame.Godot.Native.dll')
    $taskSharedSl = Join-Path $taskDiagnostics 'streamline-vulkan'
    New-Item -ItemType Directory $taskSharedSl -Force | Out-Null
    $taskManifest = Get-Content (Join-Path $taskBaseAddon 'runtime-manifest.json') -Raw | ConvertFrom-Json
    foreach ($taskEntry in $taskManifest.files | Where-Object path -Like 'runtime/streamline-vulkan/*') {
        $taskName = Split-Path $taskEntry.path -Leaf
        $taskDestination = Join-Path $taskSharedSl $taskName
        $taskCandidates = @(
            (Join-Path $taskRoot ".local/godot/vulkan/host/artifacts/streamline-production-pcl-wsi/runtime/$taskName"),
            (Join-Path $taskRoot ".local/streamline/sdk/bin/x64/$taskName"),
            (Join-Path $taskBaseAddon $taskEntry.path)
        )
        $taskFound = $false
        foreach ($taskCandidate in $taskCandidates) {
            if ((Test-Path -LiteralPath $taskCandidate -PathType Leaf) -and (Get-FileHash -LiteralPath $taskCandidate).Hash -ieq $taskEntry.sha256) {
                Copy-RsfGodotArtifact $taskCandidate $taskDestination
                $taskFound = $true
                break
            }
        }
        if (!$taskFound -and $taskName -in @('sl.interposer.dll','sl.common.dll','sl.pcl.dll','sl.reflex.dll')) {
            throw "Missing pinned diagnostic dependency: $taskName"
        }
    }
    New-Item -ItemType Directory (Join-Path $taskAddon 'runtime/fidelityfx-vulkan') -Force | Out-Null
    New-Item -ItemType Junction -Path (Join-Path $taskAddon 'runtime/streamline-vulkan') -Target $taskSharedSl | Out-Null
    Copy-RsfGodotArtifact (Join-Path $taskDiagnostics "$Stage.dll") (Join-Path $taskAddon 'runtime/fidelityfx-vulkan/amd_fidelityfx_vk.dll')
    $taskBaseInfo = Get-Content (Join-Path $taskBase 'capture-info.json') -Raw | ConvertFrom-Json
    $taskExe = $taskBaseInfo.engine
    if ((Get-FileHash -LiteralPath $taskExe).Hash -ne $taskBaseInfo.engine_sha256) { throw 'Diagnostic engine changed.' }
    $taskNativeHash = (Get-FileHash -LiteralPath (Join-Path $taskAddon 'bin/ReScaleFrame.Godot.Native.dll')).Hash
    if ($taskNativeHash -ne $taskBaseInfo.native_sha256) { throw 'Diagnostic Native changed.' }
    $taskFirst = if ($Aged) { 40 } else { 12 }
    $taskOffset = if ($Aged) { 28 } else { 0 }
    @{diagnostic=$true; stage=$Stage; aged=$Aged.IsPresent; first_frame=$taskFirst; animation_offset=$taskOffset;
        engine=$taskExe; engine_sha256=$taskBaseInfo.engine_sha256; native_sha256=$taskNativeHash;
        sdk_sha256=(Get-FileHash -LiteralPath (Join-Path $taskDiagnostics "$Stage.dll")).Hash;
        mode='object'; geometry='box'; sr_backend=2; graphics_api='vulkan'; gpu_validation=$true;
        reference=$false; reference_msaa=$false; reset_sr_history=$false; latency_mode=0;
        captured_utc=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json | Set-Content (Join-Path $taskProject 'capture-info.json') -Encoding utf8

    function Invoke-Diagnostic([string]$Name, [string[]]$Arguments, [bool]$Foreground) {
        $taskArgs = ($Arguments | ForEach-Object { if ($_.Contains('"')) { throw 'Unexpected quote in argument.' }; '"' + $_ + '"' }) -join ' '
        $taskLog = Join-Path $taskProject "$Name.log"
        $taskErrorLog = Join-Path $taskProject "$Name.stderr"
        [uint32]$taskPolicy = 0
        $taskTimer = [Diagnostics.Stopwatch]::StartNew()
        $taskProcess = if ($Foreground) {
            [RsfSyncIsolationProcess]::Start($taskExe,$taskArgs,$taskProject,$taskLog,$taskErrorLog,$true,[ref]$taskPolicy)
        } else {
            Start-Process -FilePath $taskExe -ArgumentList $taskArgs -PassThru -WindowStyle Hidden -RedirectStandardOutput $taskLog -RedirectStandardError $taskErrorLog
        }
        $taskFocus = if ($Foreground) { New-RsfCaptureForegroundReceipt $taskProcess } else { $null }
        try {
            if ($Foreground) {
                $taskFocused = $false
                while (!$taskProcess.HasExited -and $taskTimer.Elapsed.TotalSeconds -lt 30) {
                    if (Update-RsfCaptureForeground $taskProcess $taskFocus $taskTimer 'startup') { $taskFocused = $true; break }
                    Start-Sleep -Milliseconds 500
                }
                if (!$taskFocused) { throw 'Could not establish owned diagnostic foreground.' }
                $taskModules = @($taskProcess.Modules | ForEach-Object { @{name=$_.ModuleName; path=$_.FileName} })
                @{process_id=$taskProcess.Id; effective_extension_point_policy=$taskPolicy; modules=$taskModules} | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $taskProject 'process-isolation.json')
                $taskLayer = @($taskProcess.Modules | Where-Object ModuleName -eq 'VkLayer_khronos_validation.dll')
                if ($taskLayer.Count -ne 1) { throw 'Portable validation layer missing.' }
                @{path=$taskLayer[0].FileName; sha256=(Get-FileHash $taskLayer[0].FileName).Hash; synchronization=($env:VK_VALIDATION_VALIDATE_SYNC -eq '1')} | ConvertTo-Json | Set-Content (Join-Path $taskProject 'validation-layer.json')
            }
            while (!$taskProcess.WaitForExit(500)) {
                if ($Foreground) { $null = Update-RsfCaptureForeground $taskProcess $taskFocus $taskTimer 'runtime' }
                if ($taskTimer.Elapsed.TotalSeconds -gt 60) { throw 'Diagnostic exceeded its overall 60 second deadline.' }
            }
            if ($taskProcess.ExitCode -ne 0) { throw "Diagnostic $Name exit: $($taskProcess.ExitCode)" }
            $taskText = (Get-Content $taskLog -Raw) + (Get-Content $taskErrorLog -Raw)
            if ($taskText -match '(?m)^(ERROR|SCRIPT ERROR):' -or (Get-RsfGodotVulkanValidationText $taskText) -match 'VUID-|SYNC-HAZARD|Validation Error') { throw "Diagnostic reported errors: $taskLog" }
        } finally {
            if (!$taskProcess.HasExited) { Stop-Process -Id $taskProcess.Id }
            if ($Foreground) { Save-RsfCaptureForegroundReceipt $taskFocus (Join-Path $taskProject 'foreground-maintenance.json') }
        }
    }
    Invoke-Diagnostic 'import' @('--headless','--editor','--import','--path',$taskProject,'--quit-after','30') $false
    $taskSaved = @{}
    foreach ($taskKey in @('RSF_FRAME_CAPTURE_DIR','RSF_VULKAN_FRAME_CAPTURE_DIR','RSF_FRAME_CAPTURE_FIRST','RSF_FRAME_CAPTURE_COUNT','RSF_FRAME_CAPTURE_RESET_SR','RSF_FSR2_DIAG_JITTER_SIGN')) { $taskSaved[$taskKey] = [Environment]::GetEnvironmentVariable($taskKey) }
    try {
        $env:RSF_VULKAN_FRAME_CAPTURE_DIR = $taskFrames
        Remove-Item Env:RSF_FRAME_CAPTURE_DIR,Env:RSF_FRAME_CAPTURE_RESET_SR,Env:RSF_FSR2_DIAG_JITTER_SIGN -ErrorAction SilentlyContinue
        $env:RSF_FRAME_CAPTURE_FIRST = [string]$taskFirst
        $env:RSF_FRAME_CAPTURE_COUNT = '4'
        Invoke-Diagnostic 'capture' @('--path',$taskProject,'--rendering-method','forward_plus','--rendering-driver','vulkan','--position','100,100','--gpu-validation','--quit-after','120','--',"--result=$taskProject/result.json",'--mode=object','--sr=2',"--animation-offset=$taskOffset",'--latency=0','--geometry=box') $true
    } finally { Restore-RsfGodotEnvironment $taskSaved }
    $taskReport = Get-Content (Join-Path $taskProject 'result.json') -Raw | ConvertFrom-Json
    if (!$taskReport.passed -or @(Get-ChildItem $taskFrames -Filter '*sr-output-linear*.json').Count -ne 4) { throw 'Missing complete diagnostic source outputs.' }
    Write-Output "Captured diagnostic $Stage/${taskCase}: $taskProject"
} finally { $taskLease.ReleaseMutex(); $taskLease.Dispose() }
