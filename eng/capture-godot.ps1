param(
    [Parameter(Mandatory)][string]$EngineExe,
    [Parameter(Mandatory)][string]$Label,
    [ValidateSet('object','camera','combined')][string]$Mode = 'combined',
    [ValidateSet('box','billboard')][string]$Geometry = 'box',
    [switch]$Reference,
    [switch]$ReferenceMsaa,
    [switch]$ResetSrHistory,
    [ValidateRange(4,70)][int]$FirstFrame = 12,
    [ValidateRange(0,60)][int]$AnimationOffset = 0,
    [switch]$RawProbe,
    [switch]$Mono,
    [ValidateRange(0,7)][int]$SrBackend = 0,
    [ValidateRange(0,2)][int]$LatencyMode = 0,
    [ValidateSet('d3d12','vulkan')][string]$GraphicsApi = 'd3d12',
    [switch]$GpuValidation
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
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
Copy-RsfGodotAddon (Join-Path $taskRoot '.local/godot/package/addons/rescaleframe') (Join-Path $taskProject 'addons/rescaleframe')
Add-Content -LiteralPath (Join-Path $taskProject 'project.godot') -Value "`n[rescaleframe]`nenabled=true"
$taskExe = (Resolve-Path -LiteralPath $EngineExe).Path
$taskEngineHash = (Get-FileHash -LiteralPath $taskExe -Algorithm SHA256).Hash
$taskNativeHash = (Get-FileHash -LiteralPath (Join-Path $taskProject 'addons/rescaleframe/bin/ReScaleFrame.Godot.Native.dll') -Algorithm SHA256).Hash
@{engine=$taskExe; engine_sha256=$taskEngineHash; native_sha256=$taskNativeHash;
    first_frame=$FirstFrame; animation_offset=$AnimationOffset; mode=$Mode; geometry=$Geometry; sr_backend=$SrBackend; latency_mode=$LatencyMode; mono=$Mono.IsPresent; reference=$Reference.IsPresent; raw_probe=$RawProbe.IsPresent;
    reference_msaa=$ReferenceMsaa.IsPresent; reset_sr_history=$ResetSrHistory.IsPresent; graphics_api=$GraphicsApi; gpu_validation=$GpuValidation.IsPresent;
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
    $taskProcess = Start-Process -FilePath $taskExe -ArgumentList $taskArguments -WindowStyle Normal -PassThru -RedirectStandardOutput (Join-Path $taskProject "$Name.log") -RedirectStandardError (Join-Path $taskProject "$Name.stderr")
    if ($Name -eq 'capture') {
        try { Set-RsfGodotForeground $taskProcess }
        catch { if (!$taskProcess.HasExited) { Stop-Process -Id $taskProcess.Id }; throw }
        if ($GpuValidation) {
            $taskLayer = @($taskProcess.Modules | Where-Object ModuleName -eq 'VkLayer_khronos_validation.dll')
            if ($taskLayer.Count -ne 1) { if (!$taskProcess.HasExited) { Stop-Process -Id $taskProcess.Id }; throw 'Requested validation layer did not load.' }
            @{path=$taskLayer[0].FileName; sha256=(Get-FileHash -LiteralPath $taskLayer[0].FileName).Hash} |
                ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskProject 'validation-layer.json') -Encoding utf8
        }
    }
    if (!$taskProcess.WaitForExit(60000)) { Stop-Process -Id $taskProcess.Id; throw "Capture timed out: $taskProject" }
    if ($taskProcess.ExitCode) { throw "Capture failed ($($taskProcess.ExitCode)): $taskProject/$Name.log" }
    $taskError = Get-Content -Raw -LiteralPath (Join-Path $taskProject "$Name.stderr")
    if ($taskError -match '(?m)^(ERROR|SCRIPT ERROR):') { throw "Capture emitted errors: $taskProject/$Name.stderr" }
    if ($GpuValidation -and (Get-RsfGodotVulkanValidationText $taskError) -match 'VUID-|SYNC-HAZARD|Validation Error|will produce undefined values|not created with any VkDescriptorPoolSize|(?m)^vk[A-Z][A-Za-z0-9_]+\(\):') { throw "Vulkan validation failed: $taskProject/$Name.stderr" }
}
Run-Capture @('--headless','--editor','--import','--path',$taskProject,'--quit-after','30') 'import'
$taskSaved = @{}
foreach ($taskKey in @('RSF_FRAME_CAPTURE_DIR','RSF_VULKAN_FRAME_CAPTURE_DIR','RSF_FRAME_CAPTURE_FIRST','RSF_FRAME_CAPTURE_COUNT','RSF_FRAME_CAPTURE_RESET_SR')) { $taskSaved[$taskKey] = [Environment]::GetEnvironmentVariable($taskKey) }
try {
    $env:RSF_FRAME_CAPTURE_DIR = if ($GraphicsApi -eq 'd3d12') { $taskCapture } else { $null }
    $env:RSF_VULKAN_FRAME_CAPTURE_DIR = if ($GraphicsApi -eq 'vulkan') { $taskCapture } else { $null }
    $env:RSF_FRAME_CAPTURE_FIRST = if ($Reference) { [string]($FirstFrame * 2 - 1) } else { [string]$FirstFrame }
    $env:RSF_FRAME_CAPTURE_COUNT = if ($Reference) { '8' } else { '4' }
    $env:RSF_FRAME_CAPTURE_RESET_SR = if ($ResetSrHistory) { '1' } else { $null }
    $taskRunArgs = @('--path',$taskProject,'--rendering-method','forward_plus','--rendering-driver',$GraphicsApi,'--position','100,100','--quit-after','120')
    if ($GpuValidation) { $taskRunArgs += '--gpu-validation' }
    $taskRunArgs += @('--',"--result=$taskProject/result.json","--mode=$Mode","--sr=$SrBackend","--animation-offset=$AnimationOffset","--latency=$LatencyMode","--geometry=$Geometry")
    if ($Reference) { $taskRunArgs += '--reference' }
    if ($ReferenceMsaa) { $taskRunArgs += '--reference-msaa' }
    if ($RawProbe) { $taskRunArgs += '--raw-probe' }
    Run-Capture $taskRunArgs 'capture'
} finally { Restore-RsfGodotEnvironment $taskSaved }
if ((Get-FileHash -LiteralPath $taskExe -Algorithm SHA256).Hash -ne $taskEngineHash) { throw 'Engine changed during capture; discard this run.' }
$taskGenerated = @(Get-ChildItem -LiteralPath $taskCapture -Filter '*fg-generated-before-ui*.json')
if (!$Reference -and !$taskGenerated.Count) { throw "No actual generated frame was captured: $taskCapture" }
$taskReport = Get-Content -Raw -LiteralPath (Join-Path $taskProject 'result.json') | ConvertFrom-Json
if (!$taskReport.passed) { throw "Visual fixture failed: $taskProject/result.json" }
if ($Mono -and !$taskReport.mono.passed) { throw "C# visual fixture failed: $taskProject/result.json" }
Write-Output "Captured $($taskGenerated.Count) generated frame(s) and matching guides: $taskCapture"

} finally { $taskGpuLease.ReleaseMutex(); $taskGpuLease.Dispose() }
