<#
.SYNOPSIS
Runs bounded Godot Vulkan fixtures and indexes their real images and reports.
#>
[CmdletBinding()]
param(
    [ValidateSet('standard','mono')][string[]]$Edition = @('standard','mono'),
    [ValidateSet('editor','debug','release')][string[]]$Target = @('editor','debug','release'),
    [ValidateSet('sr','fg','pcl','fg_lifecycle','visual')][string[]]$Scenario = @('sr','fg','pcl','fg_lifecycle','visual'),
    [ValidateRange(0,7)][int[]]$SrBackend = @(0,1,2,3,4,5,6,7),
    [ValidateSet(1,3,5)][int[]]$FgBackend = @(1,3,5),
    [ValidateRange(1,15)][int[]]$DlssGeneratedFrames = @(1,3),
    [ValidateRange(1,15)][int[]]$XessGeneratedFrames = @(1),
    [ValidateSet('object','camera','combined')][string[]]$Motion = @('object','camera','combined'),
    [switch]$ExpectFsr4FgUnsupported,
    [string]$ValidationLayerDirectory = $env:VK_LAYER_PATH
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
if (!$ValidationLayerDirectory) { throw 'Supply -ValidationLayerDirectory or VK_LAYER_PATH for strict GPU verification.' }
$taskLayer = (Resolve-Path -LiteralPath $ValidationLayerDirectory).ProviderPath
if (!(Test-Path -LiteralPath (Join-Path $taskLayer 'VkLayer_khronos_validation.json'))) { throw 'The selected directory has no Khronos layer manifest.' }
$taskShell = (Get-Process -Id $PID).Path
$taskPackage = Join-Path $taskRoot '.local/godot/package/addons/rescaleframe'
$taskNativeHash = (Get-FileHash -LiteralPath (Join-Path $taskPackage 'bin/ReScaleFrame.Godot.Native.dll')).Hash
$taskPayloadHash = (Get-FileHash -LiteralPath (Join-Path $taskPackage 'runtime-manifest.json')).Hash
$taskStamp = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$taskOutput = Join-Path $taskRoot ".local/godot/matrix-$taskStamp"
New-Item -ItemType Directory -Path $taskOutput | Out-Null
$taskCases = [Collections.Generic.List[object]]::new()
function Add-MatrixCase([string]$SelectedEdition, [string]$SelectedTarget, [string]$SelectedScenario,
    [int]$Sr = -1, [int]$Fg = 3, [int]$Count = 1, [string]$SelectedMotion = '', [bool]$Native = $false) {
    $taskEngine = Get-RsfGodotBuild $taskRoot $SelectedEdition $(if ($SelectedTarget -eq 'editor') { 'editor' } else { "template_$SelectedTarget" })
    $taskCases.Add([ordered]@{edition=$SelectedEdition; target=$SelectedTarget; scenario=$SelectedScenario;
        sr_backend=$Sr; fg_backend=$Fg; generated_frames=$Count; motion=$SelectedMotion; native_presentation=$Native;
        expected_engine_sha256=(Get-FileHash -LiteralPath $taskEngine).Hash; status='pending'; manifest=$null;
        log=$null; error=$null; evidence=@(); images=@()})
}
foreach ($taskEdition in $Edition) {
    foreach ($taskTarget in $Target) {
        if ('sr' -in $Scenario) { foreach ($taskSr in $SrBackend) { Add-MatrixCase $taskEdition $taskTarget 'sr' $taskSr } }
        if ('fg' -in $Scenario) { foreach ($taskFg in $FgBackend) {
            $taskCounts = if ($taskFg -eq 1) { $DlssGeneratedFrames } elseif ($taskFg -eq 5) { $XessGeneratedFrames } else { @(1) }
            foreach ($taskCount in $taskCounts) { Add-MatrixCase $taskEdition $taskTarget 'fg' -Fg $taskFg -Count $taskCount }
        } }
        if ('pcl' -in $Scenario) { Add-MatrixCase $taskEdition $taskTarget 'pcl'; Add-MatrixCase $taskEdition $taskTarget 'pcl' -Native $true }
        if ('fg_lifecycle' -in $Scenario) { Add-MatrixCase $taskEdition $taskTarget 'fg_lifecycle' -Fg 1 }
        if ($ExpectFsr4FgUnsupported) { Add-MatrixCase $taskEdition $taskTarget 'fg_admission' }
    }
    # Generated-image acceptance is a separate editor suite with fresh references.
    if ('visual' -in $Scenario) { foreach ($taskMotion in $Motion) { foreach ($taskSr in $SrBackend) { foreach ($taskFg in $FgBackend) {
        Add-MatrixCase $taskEdition 'editor' 'visual' $taskSr $taskFg 1 $taskMotion
    } } } }
}
if (!$taskCases.Count) { throw 'No cases selected.' }
function Get-MatrixLink([string]$Path) {
    return [Net.WebUtility]::HtmlEncode([IO.Path]::GetRelativePath($taskOutput, $Path).Replace('\','/'))
}
function Save-Matrix {
    $taskDocument = [ordered]@{schema_version=1; created_utc=$taskStamp; complete=(@($taskCases | Where-Object { $_.status -in @('pending','running') }).Count -eq 0);
        passed=(@($taskCases | Where-Object { $_.status -ne 'passed' }).Count -eq 0); graphics_api='vulkan';
        native_sha256=$taskNativeHash; payload_manifest_sha256=$taskPayloadHash; validation_layer_directory=$taskLayer;
        scope='Bounded functional/export checks and actual generated-image editor comparisons. No broad scene-quality, physical scanout or latency claim.';
        cases=@($taskCases.ToArray())}
    $taskTemporary = Join-Path $taskOutput 'matrix.json.tmp'
    $taskDocument | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath $taskTemporary -Encoding utf8
    Move-Item -LiteralPath $taskTemporary -Destination (Join-Path $taskOutput 'matrix.json') -Force
    $taskRows = foreach ($taskCase in $taskCases) {
        $taskLinks = @()
        if ($taskCase.log) { $taskLinks += '<a href="' + (Get-MatrixLink $taskCase.log) + '">Log</a>' }
        if ($taskCase.manifest) { $taskLinks += '<a href="' + (Get-MatrixLink $taskCase.manifest) + '">Manifest</a>' }
        foreach ($taskEvidence in $taskCase.evidence) {
            $taskLinks += '<a href="' + (Get-MatrixLink $taskEvidence) + '">' + [Net.WebUtility]::HtmlEncode([IO.Path]::GetFileName($taskEvidence)) + '</a>'
        }
        $taskImages = foreach ($taskImage in $taskCase.images) { '<a href="' + (Get-MatrixLink $taskImage) + '"><img loading="lazy" src="' + (Get-MatrixLink $taskImage) + '"></a>' }
        '<tr><td>' + $taskCase.status + '</td><td>' + $taskCase.edition + '/' + $taskCase.target + '</td><td>' + $taskCase.scenario +
            '</td><td>' + $taskCase.sr_backend + '/' + $taskCase.fg_backend + '/' + $taskCase.generated_frames + ' ' + $taskCase.motion +
            '</td><td>' + ($taskLinks -join ' · ') + '<p>' + [Net.WebUtility]::HtmlEncode($taskCase.error) + '</p>' + ($taskImages -join '') + '</td></tr>'
    }
    $taskHtml = '<!doctype html><html lang="en"><meta charset="utf-8"><title>Godot Vulkan matrix</title><style>body{font:16px system-ui;margin:24px}table{border-collapse:collapse}td,th{border:1px solid #aaa;padding:8px;vertical-align:top}img{width:240px;max-height:160px;object-fit:contain}</style>' +
        '<h1>Godot Vulkan matrix</h1><p>Actual player images and linked raw/generated-image reports accompany each case. Pending, failed and running cases prevent acceptance. Independent image comparisons cover the recorded editor fixtures.</p>' +
        '<p>Native ' + $taskNativeHash + '</p><table><tr><th>Status</th><th>Edition/target</th><th>Fixture</th><th>SR/FG/count/motion</th><th>Evidence</th></tr>' + ($taskRows -join '') + '</table></html>'
    [IO.File]::WriteAllText((Join-Path $taskOutput 'index.html'), $taskHtml)
}
$taskEnvironment = @{}
foreach ($taskName in @('VK_LAYER_PATH','VK_LOADER_LAYERS_DISABLE','VK_VALIDATION_VALIDATE_SYNC')) { $taskEnvironment[$taskName] = [Environment]::GetEnvironmentVariable($taskName) }
try {
    $env:VK_LAYER_PATH = $taskLayer
    $env:VK_LOADER_LAYERS_DISABLE = '~implicit~'
    $env:VK_VALIDATION_VALIDATE_SYNC = '1'
    Save-Matrix
    for ($taskIndex = 0; $taskIndex -lt $taskCases.Count; ++$taskIndex) {
        $taskCase = $taskCases[$taskIndex]
        $taskCase.log = Join-Path $taskOutput ("case-{0:d3}.log" -f $taskIndex)
        $taskCase.status = 'running'; Save-Matrix
        $taskValidationRoot = Join-Path $taskRoot ('.local/godot/validation/' + $taskCase.edition + $(if ($taskCase.target -ne 'editor') { '-export-' + $taskCase.target } else { '' }))
        $taskPrevious = @(if (Test-Path -LiteralPath $taskValidationRoot) { Get-ChildItem -LiteralPath $taskValidationRoot -Directory | ForEach-Object Name })
        try {
            if ((Get-FileHash -LiteralPath (Join-Path $taskPackage 'bin/ReScaleFrame.Godot.Native.dll')).Hash -ne $taskNativeHash -or
                (Get-FileHash -LiteralPath (Join-Path $taskPackage 'runtime-manifest.json')).Hash -ne $taskPayloadHash) { throw 'The staged package changed during the matrix.' }
            $taskArguments = @('-NoProfile','-File')
            if ($taskCase.scenario -eq 'visual') {
                $taskArguments += @((Join-Path $PSScriptRoot 'test-godot-visual.ps1'),'-Edition',$taskCase.edition,'-Mode',$taskCase.motion,
                    '-SrBackend',"$($taskCase.sr_backend)",'-FgBackend',"$($taskCase.fg_backend)",'-LatencyMode','1','-ReferenceMsaa')
            } else {
                $taskArguments += @((Join-Path $PSScriptRoot 'test-godot.ps1'),'-Edition',$taskCase.edition,'-EngineBridge','-Scenario',$taskCase.scenario)
                if ($taskCase.target -ne 'editor') { $taskArguments += @('-Exported','-ExportConfiguration',$taskCase.target) }
                if ($taskCase.scenario -eq 'sr') {
                    $taskArguments += @('-SrBackend',"$($taskCase.sr_backend)")
                    if ($taskCase.sr_backend -notin @(0,6)) { $taskArguments += '-CaptureFrames' }
                }
                if ($taskCase.scenario -in @('fg','fg_lifecycle')) { $taskArguments += @('-FgBackend',"$($taskCase.fg_backend)",'-GeneratedFrames',"$($taskCase.generated_frames)") }
                if ($taskCase.scenario -eq 'pcl') { $taskArguments += '-NoCSharp' }
                if ($taskCase.native_presentation) { $taskArguments += '-NativeVulkanPresentation' }
            }
            $taskArguments += @('-GraphicsApi','vulkan','-GpuValidation','-DisableLegacyExtensionPoints','-MaintainForeground')
            & $taskShell @taskArguments *> $taskCase.log
            $taskExit = $LASTEXITCODE
            $taskText = Get-Content -Raw -LiteralPath $taskCase.log
            $taskMatch = [regex]::Match($taskText, '(?m)^(?:Validation manifest|Visual test manifest):\s*(.+?)\s*$')
            if ($taskMatch.Success) { $taskCase.manifest = $taskMatch.Groups[1].Value.Trim() }
            if ($taskExit) { throw "Fixture process exited $taskExit; inspect its log and retained images." }
            if (!$taskCase.manifest) { throw 'The child did not produce an acceptance manifest.' }
            $taskManifest = Get-Content -Raw -LiteralPath $taskCase.manifest | ConvertFrom-Json
            if (!$taskManifest.passed -or !$taskManifest.cases.Count) { throw 'The child manifest contains failed or missing cases.' }
            foreach ($taskResult in $taskManifest.cases) {
                # Functional manifests call the executable hash sha256; visual ones name it engine_sha256.
                $taskActualEngineHash = if ($taskCase.scenario -eq 'visual') { $taskResult.engine_sha256 } else { $taskResult.sha256 }
                if ($taskResult.native_sha256 -ne $taskNativeHash -or $taskActualEngineHash -ne $taskCase.expected_engine_sha256) { throw 'A child used different engine/native binaries.' }
                if ($taskCase.scenario -ne 'visual' -and $taskResult.payload_manifest_sha256 -ne $taskPayloadHash) { throw 'A child used a different SDK payload.' }
            }
            $taskCase.status = 'passed'
        } catch { $taskCase.status = 'failed'; $taskCase.error = $_.Exception.Message }
        if ($taskCase.scenario -eq 'visual' -and $taskCase.manifest) {
            $taskManifest = Get-Content -Raw -LiteralPath $taskCase.manifest | ConvertFrom-Json
            foreach ($taskResult in $taskManifest.cases) {
                $taskReport = Join-Path (Split-Path -Parent $taskResult.analysis) 'report.html'
                if (Test-Path -LiteralPath $taskReport) { $taskCase.evidence += $taskReport }
            }
        } elseif (Test-Path -LiteralPath $taskValidationRoot) {
            foreach ($taskFixture in Get-ChildItem -LiteralPath $taskValidationRoot -Directory | Where-Object { $_.Name -notin $taskPrevious }) {
                $taskCase.images += @(Get-ChildItem -LiteralPath $taskFixture.FullName -Filter '*.png' -File | Sort-Object Name | Select-Object -First 3 -ExpandProperty FullName)
                foreach ($taskRelative in @('vulkan.json','frames/decoded/report.html')) {
                    $taskEvidence = Join-Path $taskFixture.FullName $taskRelative
                    if (Test-Path -LiteralPath $taskEvidence) { $taskCase.evidence += $taskEvidence }
                }
            }
        }
        Save-Matrix
        Write-Output ("{0}/{1}: {2} {3}/{4} {5} SR{6} FG{7} count{8}" -f ($taskIndex + 1),$taskCases.Count,$taskCase.status,$taskCase.edition,$taskCase.target,$taskCase.scenario,$taskCase.sr_backend,$taskCase.fg_backend,$taskCase.generated_frames)
    }
} finally { Restore-RsfGodotEnvironment $taskEnvironment; Save-Matrix }
Write-Output "Matrix report: $taskOutput/index.html"
if (@($taskCases | Where-Object { $_.status -ne 'passed' }).Count) { throw 'The matrix has failed or incomplete cases. Open its report.' }
