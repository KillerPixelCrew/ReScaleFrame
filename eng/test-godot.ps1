param(
    [ValidateSet('standard','mono')][string[]]$Edition = @('standard','mono'),
    [ValidateSet('d3d12','vulkan')][string[]]$GraphicsApi = @('d3d12','vulkan'),
    [switch]$EngineBridge,
    [switch]$Exported,
    [ValidateSet('debug','release')][string]$ExportConfiguration = 'release',
    [ValidateSet('runtime','controller','pcl')][string]$Scenario = 'runtime',
    [switch]$NoCSharp
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
$taskLocal = Join-Path $taskRoot '.local/godot'
$taskSource = Join-Path $taskRoot 'integrations/godot'
$taskLock = Get-Content -Raw -LiteralPath (Join-Path $taskSource 'dependencies.json') | ConvertFrom-Json
$taskPackage = Join-Path $taskLocal 'package/addons/rescaleframe'
$taskRunStamp = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$taskRunResults = @()
if ($EngineBridge -and !$PSBoundParameters.ContainsKey('GraphicsApi')) { $GraphicsApi = @('d3d12') }
if ($EngineBridge -and $GraphicsApi -contains 'vulkan') { throw 'Godot Vulkan runtime support is unfinished. Use d3d12 for runtime tests, or omit -EngineBridge for Vulkan renderer probes.' }
if ($Exported -and !$EngineBridge) { throw 'Exported runtime validation requires -EngineBridge.' }
if ($Scenario -eq 'controller' -and !$EngineBridge) { throw 'Controller validation requires -EngineBridge.' }
if ($Scenario -eq 'pcl' -and (!$EngineBridge -or !$NoCSharp)) { throw 'The camera-free PCL fixture requires -EngineBridge -NoCSharp. C# is covered by the runtime/controller fixtures.' }
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
        $taskExe = $taskPlayer
    }
    foreach ($taskApi in $GraphicsApi) {
        $taskResult = Join-Path $taskProject "$taskApi.json"
        if (Test-Path -LiteralPath $taskResult) { Remove-Item -LiteralPath $taskResult }
        $taskRunArguments = @('--rendering-method','forward_plus','--rendering-driver',$taskApi,'--resolution','640x360')
        if (!$Exported) { $taskRunArguments = @('--path',$taskProject) + $taskRunArguments }
        if ($EngineBridge) { $taskRunArguments += @('--position','100,100','--quit-after','1200') }
        else { $taskRunArguments += @('--position','-2000,-2000','--no-focus','--fixed-fps','60','--quit-after','240') }
        $taskRunArguments += @('--',"--result=$taskResult")
        $taskCaptureSaved = @{}
        if ($Scenario -eq 'controller') {
            foreach ($taskKey in @('RSF_FRAME_CAPTURE_DIR','RSF_FRAME_CAPTURE_FIRST','RSF_FRAME_CAPTURE_COUNT')) { $taskCaptureSaved[$taskKey] = [Environment]::GetEnvironmentVariable($taskKey) }
            $env:RSF_FRAME_CAPTURE_DIR = Join-Path $taskProject 'frames'
            $env:RSF_FRAME_CAPTURE_FIRST = '95'
            $env:RSF_FRAME_CAPTURE_COUNT = '8'
        }
        try { Invoke-BoundedGodot $taskExe $taskRunArguments (Join-Path $taskProject "$taskApi.log") $EngineBridge.IsPresent }
        finally { foreach ($taskKey in $taskCaptureSaved.Keys) { [Environment]::SetEnvironmentVariable($taskKey, $taskCaptureSaved[$taskKey]) } }
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
        if ($taskEdition -eq 'mono' -and !$NoCSharp -and !$taskReport.mono.passed) { throw "Godot C# validation failed: $taskResult" }
        $taskRunResults += @{edition=$taskEdition; api=$taskApi; scenario=$Scenario; exported=$Exported.IsPresent;
            csharp=($taskEdition -eq 'mono' -and !$NoCSharp); result=$taskResult; executable=$taskExe;
            sha256=(Get-FileHash -LiteralPath $taskExe -Algorithm SHA256).Hash}
        if ($Exported) {
            @{passed=$true; edition=$taskEdition; configuration=$ExportConfiguration; executable=$taskExe;
                sha256=(Get-FileHash -LiteralPath $taskExe -Algorithm SHA256).Hash; result=$taskResult} |
                ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskExportRoot 'validation.json') -Encoding utf8
        }
        $taskScope = if ($Scenario -eq 'pcl') { 'PCL from startup without a camera or compositor' } elseif ($Scenario -eq 'controller') { 'settings, pause/cut, Off/resume, controller replacement and PCL' } elseif ($EngineBridge) { 'SR/FG/MFG switching, resize, PCL and input markers' } else { 'object/camera motion, resize, disable/resume' }
        Write-Output "PASS: Godot $taskEdition / $taskApi ($taskScope)"
    }
}
@{passed=$true; cases=$taskRunResults} | ConvertTo-Json -Depth 6 |
    Set-Content -LiteralPath (Join-Path $taskLocal "validation-$taskRunStamp.json") -Encoding utf8
Write-Output "Validation manifest: $taskLocal/validation-$taskRunStamp.json"
