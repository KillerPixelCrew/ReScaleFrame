param(
    [ValidateSet('standard','mono')][string[]]$Edition = @('standard','mono'),
    [ValidateSet('d3d12','vulkan')][string[]]$GraphicsApi = @('d3d12','vulkan'),
    [switch]$EngineBridge,
    [switch]$Exported,
    [ValidateSet('debug','release')][string]$ExportConfiguration = 'release',
    [ValidateSet('runtime','controller','pcl','sr','fg')][string]$Scenario = 'runtime',
    [switch]$NoCSharp,
    [switch]$GpuValidation,
    [switch]$CaptureFrames,
    [switch]$NativeVulkanPresentation,
    [switch]$MissingSrRuntime,
    [ValidateSet(1,2,3)][int]$SrBackend = 2
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
$taskGpuLease = Enter-RsfGodotValidation
try {
$taskLocal = Join-Path $taskRoot '.local/godot'
$taskSource = Join-Path $taskRoot 'integrations/godot'
$taskLock = Get-Content -Raw -LiteralPath (Join-Path $taskSource 'dependencies.json') | ConvertFrom-Json
$taskPackage = Join-Path $taskLocal 'package/addons/rescaleframe'
$taskRunStamp = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$taskRunResults = @()
if ($EngineBridge -and !$PSBoundParameters.ContainsKey('GraphicsApi')) { $GraphicsApi = @('d3d12') }
if ($EngineBridge -and $GraphicsApi -contains 'vulkan' -and $Scenario -notin @('sr','fg','pcl')) { throw 'Godot Vulkan full provider switching is unfinished. Use -Scenario sr/fg/pcl for native Vulkan fixtures, or omit -EngineBridge for renderer probes.' }
if ($Exported -and !$EngineBridge) { throw 'Exported runtime validation requires -EngineBridge.' }
if ($NativeVulkanPresentation -and (!$EngineBridge -or $Scenario -ne 'pcl' -or $GraphicsApi.Count -ne 1 -or $GraphicsApi[0] -ne 'vulkan')) { throw 'Native presentation control requires -EngineBridge -GraphicsApi vulkan -Scenario pcl.' }
if ($Scenario -eq 'controller' -and !$EngineBridge) { throw 'Controller validation requires -EngineBridge.' }
if ($Scenario -eq 'pcl' -and (!$EngineBridge -or !$NoCSharp)) { throw 'The camera-free PCL fixture requires -EngineBridge -NoCSharp. C# is covered by the runtime/controller fixtures.' }
if ($GpuValidation -and ($GraphicsApi.Count -ne 1 -or $GraphicsApi[0] -ne 'vulkan')) { throw 'GPU validation currently requires an explicit Vulkan-only test.' }
if ($CaptureFrames -and (!$EngineBridge -or $Scenario -notin @('sr','fg') -or $GraphicsApi.Count -ne 1 -or $GraphicsApi[0] -ne 'vulkan')) { throw 'Raw readback requires -EngineBridge -GraphicsApi vulkan -Scenario sr/fg.' }
if ($MissingSrRuntime -and (!$EngineBridge -or $Scenario -ne 'sr' -or !$NoCSharp -or $Exported -or $CaptureFrames -or $SrBackend -eq 1 -or $GraphicsApi.Count -ne 1 -or $GraphicsApi[0] -ne 'vulkan')) { throw 'Missing-runtime recovery requires -EngineBridge -Scenario sr -SrBackend 2/3 -GraphicsApi vulkan -NoCSharp, without export/raw capture.' }
if (!(Test-Path -LiteralPath (Join-Path $taskPackage 'bin/ReScaleFrame.Godot.Native.dll'))) {
    throw 'Run eng/build-godot.ps1 before validation.'
}

function Invoke-BoundedGodot([string]$Executable, [string[]]$Arguments, [string]$Log, [bool]$Foreground = $false, [int]$TimeoutSeconds = 60) {
    # Arguments are individually quoted for Start-Process, including checkout paths with spaces.
    $taskArguments = ($Arguments | ForEach-Object {
        if ($_.Contains('"')) { throw 'Unexpected quote in a Godot argument.' }
        '"' + $_ + '"'
    }) -join ' '
    $taskStyle = if ($Foreground) { 'Normal' } else { 'Hidden' }
    $taskTimer = [Diagnostics.Stopwatch]::StartNew()
    $taskProcess = Start-Process -FilePath $Executable -ArgumentList $taskArguments -PassThru -WindowStyle $taskStyle -RedirectStandardOutput $Log -RedirectStandardError ($Log + '.stderr')
    if ($Foreground) {
        try { Set-RsfGodotForeground $taskProcess }
        catch { if (!$taskProcess.HasExited) { Stop-Process -Id $taskProcess.Id }; throw }
        if ($GpuValidation) {
            $taskLayer = @($taskProcess.Modules | Where-Object ModuleName -eq 'VkLayer_khronos_validation.dll')
            if ($taskLayer.Count -ne 1) {
                if (!$taskProcess.HasExited) { Stop-Process -Id $taskProcess.Id }
                throw 'Requested Vulkan validation layer was not loaded in the test process.'
            }
            @{path=$taskLayer[0].FileName; sha256=(Get-FileHash -LiteralPath $taskLayer[0].FileName).Hash;
                synchronization=($env:VK_VALIDATION_VALIDATE_SYNC -eq '1');
                layer_filter=$env:VK_LOADER_LAYERS_DISABLE;
                modules=@($taskProcess.Modules | Where-Object ModuleName -Match '(?i)RTSS|vkhdr|VkLayer|GalaxyOverlay|nvoglv' | ForEach-Object { @{name=$_.ModuleName; path=$_.FileName} })} |
                ConvertTo-Json -Depth 4 | Set-Content -LiteralPath ($Log + '.validation-layer.json') -Encoding utf8
        }
    }
    while (!$taskProcess.WaitForExit(1000)) {
        if ($taskTimer.Elapsed.TotalSeconds -gt $TimeoutSeconds) {
            Stop-Process -Id $taskProcess.Id
            throw "Godot validation exceeded $TimeoutSeconds seconds: $Log"
        }
    }
    if ($taskProcess.ExitCode -ne 0) {
        Get-Content -LiteralPath $Log,($Log + '.stderr')
        throw "Godot validation failed ($($taskProcess.ExitCode)): $Log"
    }
    $taskErrors = Get-Content -Raw -LiteralPath ($Log + '.stderr')
    if ($taskErrors -match '(?m)^(SCRIPT ERROR|ERROR):') {
        throw "Godot emitted errors despite a successful exit: $Log.stderr"
    }
    $taskValidationText = Get-RsfGodotVulkanValidationText ((Get-Content -Raw -LiteralPath $Log) + $taskErrors)
    if ($GpuValidation -and $taskValidationText -match 'VUID-|SYNC-HAZARD|Validation Error|will produce undefined values|not created with any VkDescriptorPoolSize' -or ($GpuValidation -and $taskErrors -match '(?m)^vk[A-Z][A-Za-z0-9_]+\(\):')) {
        throw "Vulkan validation reported API errors: $Log"
    }
    if ($Foreground -and $GpuValidation -and $env:VK_VALIDATION_VALIDATE_SYNC -eq '1' -and (Get-Content -Raw -LiteralPath $Log) -notmatch '(?m)^\s+- Synchronization\s*$') {
        throw 'Synchronization validation was requested but the loaded layer did not report it enabled.'
    }
}

foreach ($taskEdition in $Edition) {
    $taskArchive = $taskLock.archives | Where-Object edition -eq $taskEdition
    $taskExe = Join-Path (Join-Path $taskLocal $taskArchive.directory) $taskArchive.executable
    if ($EngineBridge) {
        $taskExe = Get-RsfGodotBuild $taskRoot $taskEdition
    }
    # Own the engine process directly. The console wrapper launches a second process;
    # redirected hidden launches proved unreliable and cannot provide bounded PID ownership.
    $taskExe = $taskExe.Replace('_console.exe','.exe').Replace('.console.exe','.exe')
    if (!(Test-Path -LiteralPath $taskExe)) { throw 'Run eng/prepare-godot.ps1 first.' }
    $taskVariant = if ($Exported) { "$taskEdition-export-$ExportConfiguration" } else { $taskEdition }
    $taskProject = Join-Path $taskLocal "validation/$taskVariant/$Scenario-$taskRunStamp"
    New-Item -ItemType Directory -Force -Path $taskProject,(Join-Path $taskProject 'addons') | Out-Null
    foreach ($taskFile in @('project.godot','main.tscn','main.gd')) {
        Copy-Item -LiteralPath (Join-Path $taskSource "demo/$taskFile") -Destination $taskProject -Force
    }
    if ($EngineBridge) {
        Copy-Item -LiteralPath (Join-Path $taskSource 'demo/main.gd') -Destination (Join-Path $taskProject 'scene.gd') -Force
        Copy-Item -LiteralPath (Join-Path $taskSource "demo/$Scenario.gd") -Destination (Join-Path $taskProject 'main.gd') -Force
        Add-Content -LiteralPath (Join-Path $taskProject 'project.godot') -Value "`n[rescaleframe]`nenabled=true"
    }
    Copy-RsfGodotAddon $taskPackage (Join-Path $taskProject 'addons/rescaleframe')
    if ($MissingSrRuntime) {
        # Damage only this newly created fixture's private dependency copy.
        Remove-Item -LiteralPath (Join-Path $taskProject 'addons/rescaleframe/runtime/fidelityfx-vulkan/amd_fidelityfx_vk.dll')
    }
    if ($taskEdition -eq 'mono' -and !$NoCSharp) {
        Copy-Item -Path (Join-Path $taskSource 'demo/mono/*') -Destination $taskProject -Force
        $taskNuget = Join-Path (Split-Path -Parent $taskExe) 'GodotSharp/Tools/nupkgs'
        $taskNugetXml = [Security.SecurityElement]::Escape($taskNuget)
        [IO.File]::WriteAllText((Join-Path $taskProject 'NuGet.Config'), "<configuration><packageSources><clear/><add key=`"godot-local`" value=`"$taskNugetXml`"/></packageSources></configuration>`n")
        # SDK resolution happens before restore; keep the bundled SDK available without
        # changing machine-wide NuGet sources or environment configuration.
        & dotnet build (Join-Path $taskProject 'Validation.csproj') --nologo
        if ($LASTEXITCODE) { throw 'Godot C# example failed to build.' }
        Add-Content -LiteralPath (Join-Path $taskProject 'project.godot') -Value "`n[dotnet]`nproject/assembly_name=`"Validation`""
    }
    Invoke-BoundedGodot $taskExe @('--headless','--editor','--import','--path',$taskProject,'--quit-after','30') (Join-Path $taskProject 'import.log')
    if ($Exported) {
        $taskTemplateSuffix = if ($taskEdition -eq 'mono') { '.mono' } else { '' }
        $taskTemplate = (Get-RsfGodotBuild $taskRoot $taskEdition "template_$ExportConfiguration").Replace('\','/')
        $taskDebugTemplate = $taskTemplate
        $taskReleaseTemplate = $taskTemplate
        $taskSelectedTemplate = if ($ExportConfiguration -eq 'debug') { $taskDebugTemplate } else { $taskReleaseTemplate }
        if (!(Test-Path -LiteralPath $taskSelectedTemplate)) { throw "Build the matching Godot template first: $taskSelectedTemplate" }
        $taskExportRoot = Join-Path $taskLocal "exports/$taskEdition/$ExportConfiguration-$([DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff'))"
        New-Item -ItemType Directory -Path $taskExportRoot -Force | Out-Null
        $taskPlayer = Join-Path $taskExportRoot 'ReScaleFrameValidation.exe'
        $taskPreset = @"
[preset.0]
name="ReScaleFrame Windows"
platform="Windows Desktop"
runnable=true
export_filter="all_resources"
include_filter=""
exclude_filter="addons/rescaleframe/runtime/*,addons/rescaleframe/bin/*"
export_path=""

[preset.0.options]
custom_template/debug="$taskDebugTemplate"
custom_template/release="$taskReleaseTemplate"
binary_format/architecture="x86_64"
binary_format/embed_pck=false
codesign/enable=false
application/modify_resources=false
"@
        [IO.File]::WriteAllText((Join-Path $taskProject 'export_presets.cfg'), $taskPreset.Replace("`r`n", "`n"))
        Invoke-BoundedGodot $taskExe @('--headless','--path',$taskProject,"--export-$ExportConfiguration",'ReScaleFrame Windows',$taskPlayer) (Join-Path $taskProject 'export.log') $false 180
        # The exported player must carry its own vendor payload, not borrow the project.
        $taskRuntimeManifest = Get-Content -Raw -LiteralPath (Join-Path $taskExportRoot 'addons/rescaleframe/runtime-manifest.json') | ConvertFrom-Json
        foreach ($taskFile in $taskRuntimeManifest.files) {
            $taskExportedFile = Join-Path $taskExportRoot ('addons/rescaleframe/' + $taskFile.path)
            if (!(Test-Path -LiteralPath $taskExportedFile) -or (Get-FileHash -LiteralPath $taskExportedFile -Algorithm SHA256).Hash -ine $taskFile.sha256) {
                throw "Exported dependency is missing or changed: $taskExportedFile"
            }
        }
        $taskExpectedNative = (Get-FileHash -LiteralPath (Join-Path $taskProject 'addons/rescaleframe/bin/ReScaleFrame.Godot.Native.dll')).Hash
        foreach ($taskNativeCopy in @('ReScaleFrame.Godot.Native.dll','addons/rescaleframe/bin/ReScaleFrame.Godot.Native.dll')) {
            $taskNativeFile = Join-Path $taskExportRoot $taskNativeCopy
            if (!(Test-Path -LiteralPath $taskNativeFile) -or (Get-FileHash -LiteralPath $taskNativeFile).Hash -ne $taskExpectedNative) {
                throw "Exported native library differs from the tested package: $taskNativeFile"
            }
        }
        $taskExe = $taskPlayer
    }
    foreach ($taskApi in $GraphicsApi) {
        $taskResult = Join-Path $taskProject "$taskApi.json"
        if (Test-Path -LiteralPath $taskResult) { Remove-Item -LiteralPath $taskResult }
        $taskRunArguments = @('--rendering-method','forward_plus','--rendering-driver',$taskApi,'--resolution','640x360')
        if ($GpuValidation) { $taskRunArguments += '--gpu-validation' }
        if (!$Exported) { $taskRunArguments = @('--path',$taskProject) + $taskRunArguments }
        if ($EngineBridge) { $taskRunArguments += @('--position','100,100','--quit-after','1200') }
        else { $taskRunArguments += @('--position','-2000,-2000','--no-focus','--fixed-fps','60','--quit-after','240') }
        $taskRunArguments += @('--',"--result=$taskResult")
        if ($Scenario -eq 'sr') { $taskRunArguments += "--initial-sr=$SrBackend" }
        if ($MissingSrRuntime) { $taskRunArguments += '--expect-sr-failure' }
        $taskCaptureSaved = @{}
        if ($Scenario -eq 'fg' -and $taskApi -eq 'vulkan') {
            $taskCaptureSaved['VULKANSTORY_STREAMLINE_VERBOSE'] = [Environment]::GetEnvironmentVariable('VULKANSTORY_STREAMLINE_VERBOSE')
            $env:VULKANSTORY_STREAMLINE_VERBOSE = '1'
        }
        if ($NativeVulkanPresentation) {
            $taskCaptureSaved['RSF_GODOT_NATIVE_WSI'] = [Environment]::GetEnvironmentVariable('RSF_GODOT_NATIVE_WSI')
            $env:RSF_GODOT_NATIVE_WSI = '1'
        }
        if ($Scenario -eq 'controller') {
            foreach ($taskKey in @('RSF_FRAME_CAPTURE_DIR','RSF_FRAME_CAPTURE_FIRST','RSF_FRAME_CAPTURE_COUNT')) { $taskCaptureSaved[$taskKey] = [Environment]::GetEnvironmentVariable($taskKey) }
            $env:RSF_FRAME_CAPTURE_DIR = Join-Path $taskProject 'frames'
            $env:RSF_FRAME_CAPTURE_FIRST = '95'
            $env:RSF_FRAME_CAPTURE_COUNT = '8'
        }
        if ($CaptureFrames) {
            foreach ($taskKey in @('RSF_VULKAN_FRAME_CAPTURE_DIR','RSF_FRAME_CAPTURE_FIRST','RSF_FRAME_CAPTURE_COUNT')) { $taskCaptureSaved[$taskKey] = [Environment]::GetEnvironmentVariable($taskKey) }
            $env:RSF_VULKAN_FRAME_CAPTURE_DIR = Join-Path $taskProject 'frames'
            $env:RSF_FRAME_CAPTURE_FIRST = '30'
            $env:RSF_FRAME_CAPTURE_COUNT = '4'
        }
        try { Invoke-BoundedGodot $taskExe $taskRunArguments (Join-Path $taskProject "$taskApi.log") $EngineBridge.IsPresent }
        finally { Restore-RsfGodotEnvironment $taskCaptureSaved }
        if ($Scenario -eq 'controller') {
            $taskCutFiles = @(Get-ChildItem -LiteralPath (Join-Path $taskProject 'frames') -Filter '*-100-fg-depth-0.json')
            if ($taskCutFiles.Count -ne 1) { throw 'The camera-cut source evidence is missing.' }
            $taskCut = Get-Content -Raw -LiteralPath $taskCutFiles[0].FullName | ConvertFrom-Json
            if (!($taskCut.flags -band 1)) { throw 'The camera cut did not invalidate temporal history.' }
            if (@(Get-ChildItem -LiteralPath (Join-Path $taskProject 'frames') -Filter '*-100-fg-generated*.json').Count) {
                throw 'The history-cut source was interpolated instead of being presented directly.'
            }
            $taskResumeFiles = @(Get-ChildItem -LiteralPath (Join-Path $taskProject 'frames/metadata') -Filter '*-101-fsr-prepare.json')
            if ($taskResumeFiles.Count -ne 1) { throw 'The post-cut SDK dispatch evidence is missing.' }
            $taskResume = Get-Content -Raw -LiteralPath $taskResumeFiles[0].FullName | ConvertFrom-Json
            if (!$taskResume.reset -or $taskResume.result -ne 0) { throw 'FSR history did not reset before interpolation resumed.' }
            $taskSwitchFiles = @(Get-ChildItem -LiteralPath (Join-Path $taskProject 'frames') -Filter '*-95-fg-depth-0.json')
            if ($taskSwitchFiles.Count -ne 1 -or !((Get-Content -Raw -LiteralPath $taskSwitchFiles[0].FullName | ConvertFrom-Json).flags -band 1)) {
                throw 'Changing Camera3D identity did not invalidate history.'
            }
            $taskBeginFile = @(Get-ChildItem -LiteralPath (Join-Path $taskProject 'frames/metadata') -Filter '*-99-render-submit-begin.json')
            $taskSrFile = @(Get-ChildItem -LiteralPath (Join-Path $taskProject 'frames/metadata') -Filter '*-99-native-sr-begin.json')
            if ($taskBeginFile.Count -ne 1 -or $taskSrFile.Count -ne 1) { throw 'The render-submit timing evidence is missing.' }
            $taskBegin = Get-Content -Raw -LiteralPath $taskBeginFile[0].FullName | ConvertFrom-Json
            $taskSr = Get-Content -Raw -LiteralPath $taskSrFile[0].FullName | ConvertFrom-Json
            if ($taskBegin.result -ne 0 -or $taskBegin.cpu_ticks -ge $taskSr.cpu_ticks -or $taskBegin.session -ne $taskSr.session) {
                throw 'Render-submit start was not recorded before the native SR work for this source.'
            }
            $taskNextBeginFile = @(Get-ChildItem -LiteralPath (Join-Path $taskProject 'frames/metadata') -Filter '*-100-render-submit-begin.json')
            if ($taskNextBeginFile.Count -ne 1) { throw 'The time-scale timing evidence is missing.' }
            $taskNextBegin = Get-Content -Raw -LiteralPath $taskNextBeginFile[0].FullName | ConvertFrom-Json
            $taskWallDelta = ($taskNextBegin.cpu_ticks - $taskBegin.cpu_ticks) * 1000.0 / $taskBegin.cpu_tick_frequency
            if ($taskCut.frame_time_ms -lt $taskWallDelta * 0.5 -or $taskCut.frame_time_ms -gt $taskWallDelta * 2.0) {
                throw 'The temporal input duration follows scaled simulation time instead of physical frame cadence.'
            }
        }
        if (!(Test-Path -LiteralPath $taskResult)) { throw "Godot did not produce a report: $taskResult" }
        $taskReport = Get-Content -Raw -LiteralPath $taskResult | ConvertFrom-Json
        if (!$taskReport.passed) { throw "Godot validation failed: $taskResult" }
        if ($Scenario -eq 'fg' -and $taskApi -eq 'vulkan' -and $env:RSF_GODOT_SINGLE_WINDOW -ne '1') {
            $taskSdkLog = Get-Content -Raw -LiteralPath (Join-Path $taskProject "$taskApi.log.stderr")
            $taskPrimaryOwners = @([regex]::Matches($taskSdkLog, 'RSF Reflex primary swapchain selected: ([0-9A-Fa-fx]+)'))
            $taskSecondaryOwners = @([regex]::Matches($taskSdkLog, 'RSF Reflex secondary swapchain ignored: ([0-9A-Fa-fx]+) owner=([0-9A-Fa-fx]+)'))
            if ($taskPrimaryOwners.Count -lt 3 -or !$taskSecondaryOwners.Count) { throw 'Reflex primary/secondary swapchain ownership evidence is missing.' }
            $taskKnownOwners = @($taskPrimaryOwners | ForEach-Object { $_.Groups[1].Value })
            foreach ($taskSecondaryOwner in $taskSecondaryOwners) {
                if ($taskSecondaryOwner.Groups[1].Value -eq $taskSecondaryOwner.Groups[2].Value -or $taskSecondaryOwner.Groups[2].Value -notin $taskKnownOwners) { throw 'Reflex sleep ownership moved to a secondary swapchain.' }
            }
        }
        if ($NativeVulkanPresentation -and $taskReport.status.fg_available -ne 0) { throw 'The native presentation PCL control unexpectedly created an FG owner.' }
        if ($CaptureFrames -and $Scenario -eq 'sr') {
            & (Join-Path $taskLocal 'build-env/Scripts/python.exe') (Join-Path $taskRoot 'tools/analyze-godot-vulkan-sr.py') (Join-Path $taskProject 'frames') --report $taskResult
            if ($LASTEXITCODE) { throw "Vulkan raw frame checks failed: $taskProject/frames/decoded/report.html" }
        }
        if ($taskEdition -eq 'mono' -and !$NoCSharp -and !$taskReport.mono.passed) { throw "Godot C# validation failed: $taskResult" }
        $taskRunResults += @{edition=$taskEdition; api=$taskApi; scenario=$Scenario; initial_sr_backend=$(if ($Scenario -eq 'sr') { $SrBackend } else { $null }); exported=$Exported.IsPresent; gpu_validation=$GpuValidation.IsPresent; raw_frames=$CaptureFrames.IsPresent; native_presentation=$NativeVulkanPresentation.IsPresent; missing_sr_runtime=$MissingSrRuntime.IsPresent;
            csharp=($taskEdition -eq 'mono' -and !$NoCSharp); result=$taskResult; executable=$taskExe;
            sha256=(Get-FileHash -LiteralPath $taskExe -Algorithm SHA256).Hash;
            native_sha256=(Get-FileHash -LiteralPath $(if ($Exported) { Join-Path $taskExportRoot 'addons/rescaleframe/bin/ReScaleFrame.Godot.Native.dll' } else { Join-Path $taskProject 'addons/rescaleframe/bin/ReScaleFrame.Godot.Native.dll' })).Hash;
            payload_manifest_sha256=if (Test-Path -LiteralPath (Join-Path $taskProject 'addons/rescaleframe/runtime-manifest.json')) { (Get-FileHash -LiteralPath (Join-Path $taskProject 'addons/rescaleframe/runtime-manifest.json')).Hash } else { $null }}
        if ($Exported) {
            @{passed=$true; edition=$taskEdition; configuration=$ExportConfiguration; executable=$taskExe;
                sha256=(Get-FileHash -LiteralPath $taskExe -Algorithm SHA256).Hash; result=$taskResult} |
                ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskExportRoot 'validation.json') -Encoding utf8
        }
        $taskScope = if ($MissingSrRuntime) { 'missing Vulkan SR DLL, preserved provider refusal and visible spatial output through provider switches/resize' } elseif ($Scenario -eq 'fg') { 'FSR3 FG callbacks and PCL, SR-independent generation, Off/resume and resize; no image-quality claim' } elseif ($Scenario -eq 'sr') { "SR $SrBackend to SR $(if ($SrBackend -eq 1) { 3 } else { 5 - $SrBackend }), resize, reset and actual player PNGs; no FG/PCL claim" } elseif ($Scenario -eq 'pcl') { 'PCL from startup without a camera or compositor' } elseif ($Scenario -eq 'controller') { 'settings, pause/cut, Off/resume, controller replacement and PCL' } elseif ($EngineBridge) { 'SR/FG/MFG switching, resize, PCL and input markers' } else { 'object/camera motion, resize, disable/resume' }
        Write-Output "PASS: Godot $taskEdition / $taskApi ($taskScope)"
    }
}
@{passed=$true; cases=$taskRunResults} | ConvertTo-Json -Depth 6 |
    Set-Content -LiteralPath (Join-Path $taskLocal "validation-$taskRunStamp.json") -Encoding utf8
Write-Output "Validation manifest: $taskLocal/validation-$taskRunStamp.json"

} finally { $taskGpuLease.ReleaseMutex(); $taskGpuLease.Dispose() }
