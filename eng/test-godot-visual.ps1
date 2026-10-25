param(
    [ValidateSet('standard','mono')][string[]]$Edition = @('standard','mono'),
    [ValidateSet('object','camera','combined')][string[]]$Mode = @('object','camera','combined'),
    [ValidateSet('box','billboard')][string]$Geometry = 'box',
    [ValidateRange(0,7)][int[]]$SrBackend = @(0,1,2,3,5),
    [ValidateRange(4,70)][int]$FirstFrame = 12,
    [ValidateSet('d3d12','vulkan')][string]$GraphicsApi = 'd3d12',
    [ValidateRange(0,2)][int]$LatencyMode = 0,
    [switch]$GpuValidation,
    [switch]$ReferenceMsaa
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
if ($GraphicsApi -eq 'vulkan' -and !$PSBoundParameters.ContainsKey('SrBackend')) { $SrBackend = @(0,1,2,3) }
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
$taskPython = Join-Path $taskRoot '.local/godot/build-env/Scripts/python.exe'
$taskRun = 'visual-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$taskResults = @()
foreach ($taskEdition in $Edition) {
    $taskEngine = Get-RsfGodotBuild $taskRoot $taskEdition
    foreach ($taskMode in $Mode) {
        $taskReferenceLabel = "$taskRun-$taskEdition-reference"
        $taskOptions = @{EngineExe=$taskEngine; Mode=$taskMode; Geometry=$Geometry; FirstFrame=$FirstFrame; Mono=($taskEdition -eq 'mono'); GraphicsApi=$GraphicsApi; GpuValidation=$GpuValidation.IsPresent; LatencyMode=$LatencyMode}
        & (Join-Path $PSScriptRoot 'capture-godot.ps1') @taskOptions -Label $taskReferenceLabel -Reference -ReferenceMsaa:$ReferenceMsaa
        $taskReference = Join-Path $taskRoot ".local/godot/visual/$taskReferenceLabel-$taskMode"
        foreach ($taskSr in $SrBackend) {
            $taskLabel = "$taskRun-$taskEdition-sr$taskSr"
            $taskCapture = Join-Path $taskRoot ".local/godot/visual/$taskLabel-$taskMode"
            try {
            & (Join-Path $PSScriptRoot 'capture-godot.ps1') @taskOptions -Label $taskLabel -SrBackend $taskSr
            & $taskPython (Join-Path $taskRoot 'tools/analyze-godot-capture.py') $taskCapture --reference $taskReference --check
            if ($LASTEXITCODE) { throw "Godot generated-frame comparison failed: $taskCapture/decoded/analysis.json" }
            $taskInfo = Get-Content -Raw -LiteralPath (Join-Path $taskCapture 'capture-info.json') | ConvertFrom-Json
            $taskReferenceInfo = Get-Content -Raw -LiteralPath (Join-Path $taskReference 'capture-info.json') | ConvertFrom-Json
            if ($taskInfo.engine_sha256 -ne $taskReferenceInfo.engine_sha256 -or $taskInfo.native_sha256 -ne $taskReferenceInfo.native_sha256) {
                throw 'The generated and reference images used different binaries.'
            }
            $taskResults += @{passed=$true; edition=$taskEdition; mode=$taskMode; sr_backend=$taskSr; latency_mode=$LatencyMode; engine_sha256=$taskInfo.engine_sha256; native_sha256=$taskInfo.native_sha256;
                analysis=(Join-Path $taskCapture 'decoded/analysis.json')}
            Write-Output "PASS: actual generated frames and geometry-derived temporal inputs, Godot $taskEdition / $taskMode / SR $taskSr"
            } catch {
                $taskInfoPath = Join-Path $taskCapture 'capture-info.json'
                $taskInfo = if (Test-Path -LiteralPath $taskInfoPath) { Get-Content -Raw -LiteralPath $taskInfoPath | ConvertFrom-Json } else { $null }
                $taskResults += @{passed=$false; edition=$taskEdition; mode=$taskMode; sr_backend=$taskSr; latency_mode=$LatencyMode;
                    engine_sha256=$(if ($null -ne $taskInfo) { $taskInfo.engine_sha256 } else { $null });
                    native_sha256=$(if ($null -ne $taskInfo) { $taskInfo.native_sha256 } else { $null });
                    analysis=(Join-Path $taskCapture 'decoded/analysis.json'); error=$_.Exception.Message}
                Write-Output "FAIL: Godot $taskEdition / $taskMode / SR $taskSr : $($_.Exception.Message)"
            }
        }
    }
}
$taskManifest = Join-Path $taskRoot ".local/godot/$taskRun.json"
$taskFailed = @($taskResults | Where-Object { !$_.passed })
@{passed=($taskFailed.Count -eq 0); first_frame=$FirstFrame; geometry=$Geometry; graphics_api=$GraphicsApi; latency_mode=$LatencyMode; gpu_validation=$GpuValidation.IsPresent; reference_msaa=$ReferenceMsaa.IsPresent; cases=$taskResults} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $taskManifest -Encoding utf8
Write-Output "Visual test manifest: $taskManifest"
$taskRows = foreach ($taskCase in $taskResults) {
    $taskState = if ($taskCase.passed) { 'PASS' } else { 'FAIL' }
    $taskReport = Join-Path (Split-Path -Parent $taskCase.analysis) 'report.html'
    $taskLink = if (Test-Path -LiteralPath $taskReport) {
        $taskRelative = [IO.Path]::GetRelativePath((Split-Path -Parent $taskManifest), $taskReport).Replace('\','/')
        '<a href="' + [Net.WebUtility]::HtmlEncode($taskRelative) + '">Frames, guides and errors</a>'
    } else { 'Capture failed; inspect the manifest error.' }
    '<tr><td>' + $taskCase.edition + '</td><td>' + $taskCase.mode + '</td><td>' + $taskCase.sr_backend +
        '</td><td>' + $taskState + '</td><td>' + $taskLink + '</td></tr>'
}
$taskHtml = '<!doctype html><html lang="en"><meta charset="utf-8"><title>Godot image matrix</title>' +
    '<style>body{font:16px system-ui;margin:24px}td,th{padding:8px;border:1px solid #999}table{border-collapse:collapse}</style>' +
    '<h1>Godot actual generated-frame matrix</h1><p>' + $taskFailed.Count + ' of ' + $taskResults.Count +
    ' cases failed. These box/floor fixtures do not establish broad image quality or latency.</p>' +
    '<table><tr><th>Edition</th><th>Motion</th><th>SR</th><th>Status</th><th>Evidence</th></tr>' +
    ($taskRows -join '') + '</table></html>'
[IO.File]::WriteAllText((Join-Path $taskRoot ".local/godot/$taskRun.html"), $taskHtml)
if ($taskFailed.Count) { throw "$($taskFailed.Count) Godot visual cases failed. Inspect $taskManifest and the captured images." }
