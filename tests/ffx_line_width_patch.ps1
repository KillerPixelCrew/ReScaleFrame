param([string]$SdkRoot = (Join-Path $PSScriptRoot '../references/godot/fidelityfx-vulkan'))
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = Split-Path -Parent $PSScriptRoot
. (Join-Path $repo 'runtime/vulkan/fsr3/sdk-fixes/line-width.ps1')
$fix = Get-RsfFfxLineWidthFix
if (!(Test-Path -LiteralPath (Join-Path $SdkRoot $fix.files[0].file))) {
    Write-Output 'SKIP: pinned FidelityFX source required for exact-file patch tests.'
    exit 77
}
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('rsf-ffx-line-width-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temporary | Out-Null
function Expect-Refusal([scriptblock]$Action, [string]$Message) {
    $refused = $false
    try { & $Action } catch { $refused = $true }
    if (!$refused) { throw "Expected refusal: $Message" }
}
try {
    foreach ($entry in $fix.files) {
        $destination = Join-Path $temporary $entry.file
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $SdkRoot $entry.file) -Destination $destination
    }
    $second = Join-Path $temporary $fix.files[1].file
    $secondBytes = [IO.File]::ReadAllBytes($second)
    [IO.File]::AppendAllText($second, '// unexpected source change')
    $firstHash = (Get-FileHash -LiteralPath (Join-Path $temporary $fix.files[0].file)).Hash
    Expect-Refusal { Set-RsfFfxLineWidth $temporary $fix.sdkCommit } 'changed second file'
    if ($firstHash -ne (Get-FileHash -LiteralPath (Join-Path $temporary $fix.files[0].file)).Hash) {
        throw 'A failed complete-source check partially applied the correction.'
    }
    [IO.File]::WriteAllBytes($second, $secondBytes)
    Expect-Refusal { Set-RsfFfxLineWidth $temporary $fix.correctionCommit } 'wrong SDK source revision'
    Set-RsfFfxLineWidth $temporary $fix.sdkCommit
    foreach ($entry in $fix.files) {
        $text = [IO.File]::ReadAllText((Join-Path $temporary $entry.file))
        if ((Get-RsfFfxLineWidthTextHash $text) -ne $entry.afterSha256) { throw 'Reviewed patch bytes differ.' }
    }
    Expect-Refusal { Set-RsfFfxLineWidth $temporary $fix.sdkCommit } 'duplicate application'
    $receipt = [pscustomobject]@{
        schema=2; sdkCommit=$fix.sdkCommit; lineWidthSourceCommit=$fix.sdkCommit
        lineWidthCorrectionCommit=$fix.correctionCommit
        lineWidthPatchSha256=(Get-FileHash -LiteralPath (Join-Path $repo 'runtime/vulkan/fsr3/sdk-fixes/line-width.json')).Hash
        lineWidthHelperSha256=(Get-FileHash -LiteralPath (Join-Path $repo 'runtime/vulkan/fsr3/sdk-fixes/line-width.ps1')).Hash
        lineWidthSources=Get-RsfFfxLineWidthSources $temporary
    }
    $json = $receipt | ConvertTo-Json -Depth 5
    Assert-RsfFfxLineWidthReceipt ($json | ConvertFrom-Json) $temporary
    foreach ($field in @('lineWidthSourceCommit','lineWidthCorrectionCommit','lineWidthPatchSha256','lineWidthHelperSha256')) {
        $bad = $json | ConvertFrom-Json
        $bad.$field = 'altered'
        Expect-Refusal { Assert-RsfFfxLineWidthReceipt $bad } "receipt identity $field"
    }
    $bad = $json | ConvertFrom-Json
    $bad.PSObject.Properties.Remove('lineWidthPatchSha256')
    Expect-Refusal { Assert-RsfFfxLineWidthReceipt $bad } 'missing correction identity'
    $bad = $json | ConvertFrom-Json
    $bad.schema = 1
    Expect-Refusal { Assert-RsfFfxLineWidthReceipt $bad } 'stale schema'
    $bad = $json | ConvertFrom-Json
    $bad.lineWidthSources[0].sourceAfterSha256 = ('0' * 64)
    Expect-Refusal { Assert-RsfFfxLineWidthReceipt $bad } 'different reviewed source bytes'
    [IO.File]::AppendAllText($second, '// stale exported source')
    Expect-Refusal { Assert-RsfFfxLineWidthReceipt $receipt $temporary } 'mutated cached export'
    # Packaging must bind both the manifest and executable patch helper.
    $packageRecipe = [IO.File]::ReadAllText((Join-Path $repo 'eng/build-godot.ps1'))
    $buildIdentity = [IO.File]::ReadAllText((Join-Path $repo 'eng/build-godot-vulkan.ps1'))
    if (!$packageRecipe.Contains('Assert-RsfFfxLineWidthReceipt $taskReceipt') -or
        !$buildIdentity.Contains('sdk-fixes/line-width.json') -or !$buildIdentity.Contains('sdk-fixes/line-width.ps1')) {
        throw 'Packaging/build identity omitted the correction.'
    }
    Write-Output 'PASS: exact source, atomic refusal, revision, duplicate, receipt and packaging line-width contracts.'
} finally {
    # Only this test-created GUID directory can be removed.
    if ([IO.Path]::GetFullPath($temporary).StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()), [StringComparison]::OrdinalIgnoreCase)) {
        Remove-Item -LiteralPath $temporary -Recurse -Force
    }
}
