param(
    [Parameter(Mandatory)][string]$EngineExe,
    [Parameter(Mandatory)][string]$Label,
    [ValidateSet('object','camera','combined')][string]$Mode = 'combined',
    [ValidateSet('box','billboard')][string]$Geometry = 'box',
    [switch]$Reference,
    [switch]$ReferenceMsaa,
    [ValidateSet(2,4)][int]$ReferenceSubsteps = 2,
    [switch]$ResetSrHistory,
    [ValidateRange(4,70)][int]$FirstFrame = 12,
    [ValidateRange(0,60)][int]$AnimationOffset = 0,
    [switch]$RawProbe,
    [switch]$Mono,
    [ValidateRange(0,7)][int]$SrBackend = 0,
    [ValidateSet(1,3,5)][int]$FgBackend = 3,
    [ValidateRange(1,15)][int]$GeneratedFrames = 1,
    [switch]$LargeOutput,
    [ValidateRange(0,2)][int]$LatencyMode = 0,
    [ValidateSet('d3d12','vulkan')][string]$GraphicsApi = 'd3d12',
    [switch]$GpuValidation,
    [switch]$DisableLegacyExtensionPoints,
    [switch]$MaintainForeground
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
if ($DisableLegacyExtensionPoints) { . (Join-Path $PSScriptRoot 'godot-process-isolation.ps1') }
if ($MaintainForeground) { . (Join-Path $PSScriptRoot 'godot-capture-foreground.ps1') }
$taskGpuLease = Enter-RsfGodotValidation
try {
$taskSource = Join-Path $taskRoot 'integrations/godot'
if ($Label -notmatch '^[a-zA-Z0-9_-]+$') { throw 'Use a simple capture label.' }
if ($ReferenceMsaa -and !$Reference) { throw 'MSAA reference sampling requires -Reference.' }
if ($ResetSrHistory -and $GraphicsApi -eq 'vulkan') { throw 'The history-reset diagnostic currently targets DX12.' }
if ($GpuValidation -and $GraphicsApi -ne 'vulkan') { throw 'GPU validation currently targets Vulkan.' }
$taskProject = Join-Path $taskRoot ".local/godot/visual/$Label-$Mode"
$taskCapture = Join-Path $taskProject 'frames'
if ((Test-Path -LiteralPath $taskProject) -and @(Get-ChildItem -LiteralPath $taskProject -Force).Count) {
    throw "Capture directory already contains evidence; choose a new label: $taskProject"
}
New-Item -ItemType Directory -Force -Path $taskProject,$taskCapture,(Join-Path $taskProject 'addons') | Out-Null
Copy-Item -LiteralPath (Join-Path $taskSource 'demo/project.godot'),(Join-Path $taskSource 'demo/main.tscn') -Destination $taskProject -Force
Copy-Item -LiteralPath (Join-Path $taskSource 'demo/main.gd') -Destination (Join-Path $taskProject 'scene.gd') -Force
Copy-Item -LiteralPath (Join-Path $taskSource 'demo/visual.gd') -Destination (Join-Path $taskProject 'main.gd') -Force
Copy-RsfGodotFixtureAddon (Join-Path $taskRoot '.local/godot/package/addons/rescaleframe') (Join-Path $taskProject 'addons/rescaleframe') (Join-Path $taskRoot '.local/godot/payload-cache')
Add-Content -LiteralPath (Join-Path $taskProject 'project.godot') -Value "`n[rescaleframe]`nenabled=true"
$taskExe = (Resolve-Path -LiteralPath $EngineExe).Path
$taskEngineHash = (Get-FileHash -LiteralPath $taskExe -Algorithm SHA256).Hash
$taskNativeHash = (Get-FileHash -LiteralPath (Join-Path $taskProject 'addons/rescaleframe/bin/ReScaleFrame.Godot.Native.dll') -Algorithm SHA256).Hash
@{engine=$taskExe; engine_sha256=$taskEngineHash; native_sha256=$taskNativeHash;
    first_frame=$FirstFrame; animation_offset=$AnimationOffset; mode=$Mode; geometry=$Geometry; sr_backend=$SrBackend; latency_mode=$LatencyMode; mono=$Mono.IsPresent; reference=$Reference.IsPresent; raw_probe=$RawProbe.IsPresent;
    fg_backend=$FgBackend; generated_frames=$GeneratedFrames;
    large_output=($LargeOutput.IsPresent -or $FgBackend -in @(1,5)); reference_substeps=$ReferenceSubsteps;
    reference_msaa=$ReferenceMsaa.IsPresent; reset_sr_history=$ResetSrHistory.IsPresent; graphics_api=$GraphicsApi; gpu_validation=$GpuValidation.IsPresent;
    disable_legacy_extension_points=$DisableLegacyExtensionPoints.IsPresent;
    maintain_foreground=$MaintainForeground.IsPresent;
    captured_utc=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskProject 'capture-info.json') -Encoding utf8
if ($Mono) {
    Copy-Item -Path (Join-Path $taskSource 'demo/mono/*') -Destination $taskProject -Force
    $taskNuget = [Security.SecurityElement]::Escape((Join-Path (Split-Path -Parent $taskExe) 'GodotSharp/Tools/nupkgs'))
    [IO.File]::WriteAllText((Join-Path $taskProject 'NuGet.Config'), "<configuration><packageSources><clear/><add key=`"godot-local`" value=`"$taskNuget`"/></packageSources></configuration>`n")
    & dotnet build (Join-Path $taskProject 'Validation.csproj') --nologo
    if ($LASTEXITCODE) { throw 'Godot C# visual fixture failed to build.' }
    Add-Content -LiteralPath (Join-Path $taskProject 'project.godot') -Value "`n[dotnet]`nproject/assembly_name=`"Validation`""
}
function Run-Capture([string[]]$Arguments, [string]$Name) {
    $taskArguments = ($Arguments | ForEach-Object { if ($_.Contains('"')) { throw 'Unexpected quote in capture argument.' }; '"' + $_ + '"' }) -join ' '
    $taskIsolated = $DisableLegacyExtensionPoints -and $Name -eq 'capture'
    [uint32]$taskProcessPolicy = 0
    if ($taskIsolated) {
        $taskProcess = [RsfSyncIsolationProcess]::Start($taskExe,$taskArguments,(Get-Location).ProviderPath,
            (Join-Path $taskProject "$Name.log"),(Join-Path $taskProject "$Name.stderr"),$true,[ref]$taskProcessPolicy)
    } else {
        $taskProcess = Start-Process -FilePath $taskExe -ArgumentList $taskArguments -WindowStyle Normal -PassThru -RedirectStandardOutput (Join-Path $taskProject "$Name.log") -RedirectStandardError (Join-Path $taskProject "$Name.stderr")
    }
    $taskMaintain = $MaintainForeground -and $Name -eq 'capture'
    $taskForegroundTimer = [Diagnostics.Stopwatch]::StartNew()
    $taskForegroundReceipt = if ($taskMaintain) { New-RsfCaptureForegroundReceipt $taskProcess } else { $null }
    try {
    if ($Name -eq 'capture') {
        try {
            if ($taskMaintain) {
                $taskFocused = $false
                while (!$taskProcess.HasExited -and $taskForegroundTimer.Elapsed.TotalSeconds -lt 30) {
                    if (Update-RsfCaptureForeground $taskProcess $taskForegroundReceipt $taskForegroundTimer 'startup') { $taskFocused = $true; break }
                    Start-Sleep -Milliseconds 500
                }
                if (!$taskFocused) { throw "Cannot establish the owned GPU fixture's foreground window: $($taskProcess.Id)" }
            } else { Set-RsfGodotForeground $taskProcess }
        }
        catch { if (!$taskProcess.HasExited) { Stop-Process -Id $taskProcess.Id }; throw }
        if ($GpuValidation) {
            $taskLayer = @($taskProcess.Modules | Where-Object ModuleName -eq 'VkLayer_khronos_validation.dll')
            if ($taskLayer.Count -ne 1) { if (!$taskProcess.HasExited) { Stop-Process -Id $taskProcess.Id }; throw 'Requested validation layer did not load.' }
            @{path=$taskLayer[0].FileName; sha256=(Get-FileHash -LiteralPath $taskLayer[0].FileName).Hash} |
                ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskProject 'validation-layer.json') -Encoding utf8
        }
    }
    if ($taskIsolated -or $taskMaintain) {
        $taskIsolationTimer = [Diagnostics.Stopwatch]::StartNew()
        $taskProcessModules = @{}
        while (!$taskProcess.HasExited) {
            if ($taskMaintain) { $null = Update-RsfCaptureForeground $taskProcess $taskForegroundReceipt $taskForegroundTimer 'runtime' }
            try { $taskLiveModules = $taskProcess.Modules }
            catch {
                if ($taskProcess.HasExited) { break }
                Stop-Process -Id $taskProcess.Id
                throw
            }
            foreach ($taskModule in $taskLiveModules) {
                if (!$taskProcessModules.ContainsKey($taskModule.FileName)) {
                    $taskProcessModules[$taskModule.FileName] = @{name=$taskModule.ModuleName; path=$taskModule.FileName}
                }
            }
            if ($taskProcess.WaitForExit(500)) { break }
            if (($taskMaintain -and $taskForegroundTimer.Elapsed.TotalSeconds -gt 60) -or (!$taskMaintain -and $taskIsolationTimer.Elapsed.TotalSeconds -gt 60)) { Stop-Process -Id $taskProcess.Id; throw "Capture timed out: $taskProject" }
            $taskProcess.Refresh()
        }
        if ($taskIsolated) { @{process_id=$taskProcess.Id; requested_extension_point_disable=$true; effective_extension_point_policy=$taskProcessPolicy;
            working_directory=(Get-Location).ProviderPath; modules=@($taskProcessModules.Values | Sort-Object name)} |
            ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskProject 'process-isolation.json') -Encoding utf8 }
    } elseif (!$taskProcess.WaitForExit(60000)) { Stop-Process -Id $taskProcess.Id; throw "Capture timed out: $taskProject" }
    if ($taskProcess.ExitCode) { throw "Capture failed ($($taskProcess.ExitCode)): $taskProject/$Name.log" }
    $taskError = Get-Content -Raw -LiteralPath (Join-Path $taskProject "$Name.stderr")
    if ($taskError -match '(?m)^(ERROR|SCRIPT ERROR):') { throw "Capture emitted errors: $taskProject/$Name.stderr" }
    if ($GpuValidation -and (Get-RsfGodotVulkanValidationText $taskError) -match 'VUID-|SYNC-HAZARD|Validation Error|will produce undefined values|not created with any VkDescriptorPoolSize|(?m)^vk[A-Z][A-Za-z0-9_]+\(\):') { throw "Vulkan validation failed: $taskProject/$Name.stderr" }
    } finally {
        if ($taskMaintain) {
            Save-RsfCaptureForegroundReceipt $taskForegroundReceipt (Join-Path $taskProject 'foreground-maintenance.json')
        }
    }
}
Run-Capture @('--headless','--editor','--import','--path',$taskProject,'--quit-after','30') 'import'
$taskSaved = @{}
foreach ($taskKey in @('RSF_FRAME_CAPTURE_DIR','RSF_VULKAN_FRAME_CAPTURE_DIR','RSF_FRAME_CAPTURE_FIRST','RSF_FRAME_CAPTURE_COUNT','RSF_FRAME_CAPTURE_RESET_SR')) { $taskSaved[$taskKey] = [Environment]::GetEnvironmentVariable($taskKey) }
try {
    $env:RSF_FRAME_CAPTURE_DIR = if ($GraphicsApi -eq 'd3d12') { $taskCapture } else { $null }
    $env:RSF_VULKAN_FRAME_CAPTURE_DIR = if ($GraphicsApi -eq 'vulkan') { $taskCapture } else { $null }
    $env:RSF_FRAME_CAPTURE_FIRST = if ($Reference) { [string]($FirstFrame * $ReferenceSubsteps - $ReferenceSubsteps + 1) } else { [string]$FirstFrame }
    $env:RSF_FRAME_CAPTURE_COUNT = if ($Reference) { [string](4 * $ReferenceSubsteps) } else { '4' }
    $env:RSF_FRAME_CAPTURE_RESET_SR = if ($ResetSrHistory) { '1' } else { $null }
    $taskRunArgs = @('--path',$taskProject,'--rendering-method','forward_plus','--rendering-driver',$GraphicsApi,'--position','100,100','--quit-after','120')
    if ($GpuValidation) { $taskRunArgs += '--gpu-validation' }
    $taskRunArgs += @('--',"--result=$taskProject/result.json","--mode=$Mode","--sr=$SrBackend","--fg=$FgBackend","--generated=$GeneratedFrames","--animation-offset=$AnimationOffset","--latency=$LatencyMode","--geometry=$Geometry")
    if ($FgBackend -in @(1,5) -or $LargeOutput) { $taskRunArgs += '--large-output' }
    if ($Reference) { $taskRunArgs += @('--reference',"--reference-substeps=$ReferenceSubsteps") }
    if ($ReferenceMsaa) { $taskRunArgs += '--reference-msaa' }
    if ($RawProbe) { $taskRunArgs += '--raw-probe' }
    Run-Capture $taskRunArgs 'capture'
} finally { Restore-RsfGodotEnvironment $taskSaved }
if ((Get-FileHash -LiteralPath $taskExe -Algorithm SHA256).Hash -ne $taskEngineHash) { throw 'Engine changed during capture; discard this run.' }
$taskGenerated = @(Get-ChildItem -LiteralPath $taskCapture -Filter '*.json' | Where-Object Name -Match 'fg-generated-(before-ui|present-)|fg-xess-generated-before-present')
if (!$Reference -and !$taskGenerated.Count) { throw "No actual generated frame was captured: $taskCapture" }
$taskReport = Get-Content -Raw -LiteralPath (Join-Path $taskProject 'result.json') | ConvertFrom-Json
if (!$taskReport.passed) { throw "Visual fixture failed: $taskProject/result.json" }
if ($Mono -and !$taskReport.mono.passed) { throw "C# visual fixture failed: $taskProject/result.json" }
Write-Output "Captured $($taskGenerated.Count) generated frame(s) and matching guides: $taskCapture"

} finally { $taskGpuLease.ReleaseMutex(); $taskGpuLease.Dispose() }
