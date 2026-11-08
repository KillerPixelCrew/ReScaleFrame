# Exact, task-owned source correction and receipt checks shared by build/staging.
function Get-RsfFfxLineWidthFix {
    $manifest = Join-Path $PSScriptRoot 'line-width.json'
    return Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json
}

function Get-RsfFfxLineWidthTextHash([string]$Text) {
    # Git blobs use LF; source exports may start with checkout CRLF bytes.
    $bytes = [Text.Encoding]::UTF8.GetBytes($Text.Replace("`r`n", "`n"))
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))
}

function Set-RsfFfxLineWidth([string]$Source, [string]$SdkCommit) {
    $fix = Get-RsfFfxLineWidthFix
    if ($SdkCommit -ne $fix.sdkCommit) { throw 'Pipeline line-width correction requires the pinned SDK commit.' }
    $edits = @()
    foreach ($entry in $fix.files) {
        $path = Join-Path $Source $entry.file
        $text = [IO.File]::ReadAllText($path).Replace("`r`n", "`n")
        if ((Get-RsfFfxLineWidthTextHash $text) -ne $entry.beforeSha256 -or
            [regex]::Matches($text, [regex]::Escape($fix.before)).Count -ne 1) {
            throw "Pinned SDK pipeline line-width source changed: $($entry.file)"
        }
        $after = $text.Replace($fix.before, $fix.after)
        if ((Get-RsfFfxLineWidthTextHash $after) -ne $entry.afterSha256) {
            throw "Reviewed SDK pipeline line-width correction differs: $($entry.file)"
        }
        $edits += @{ path=$path; text=$after }
    }
    # Validate both complete files before changing either export.
    foreach ($edit in $edits) {
        [IO.File]::WriteAllText($edit.path, $edit.text, [Text.UTF8Encoding]::new($false))
    }
}

function Get-RsfFfxLineWidthSources([string]$Source) {
    $fix = Get-RsfFfxLineWidthFix
    return @($fix.files | ForEach-Object {
        [ordered]@{ file=$_.file; sourceBeforeSha256=$_.beforeSha256; sourceAfterSha256=$_.afterSha256
            stagedSha256=(Get-FileHash -LiteralPath (Join-Path $Source $_.file) -Algorithm SHA256).Hash }
    })
}

function Assert-RsfFfxLineWidthReceipt($Receipt, [string]$Source = '') {
    $fix = Get-RsfFfxLineWidthFix
    $required = @('schema','sdkCommit','lineWidthSourceCommit','lineWidthCorrectionCommit',
        'lineWidthPatchSha256','lineWidthHelperSha256','lineWidthSources')
    foreach ($field in $required) {
        if (!$Receipt.PSObject.Properties[$field]) { throw "SDK receipt is missing $field." }
    }
    if ($Receipt.schema -ne 2 -or $Receipt.sdkCommit -ne $fix.sdkCommit -or
        $Receipt.lineWidthSourceCommit -ne $fix.sdkCommit -or
        $Receipt.lineWidthCorrectionCommit -ne $fix.correctionCommit -or
        $Receipt.lineWidthPatchSha256 -ne (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'line-width.json')).Hash -or
        $Receipt.lineWidthHelperSha256 -ne (Get-FileHash -LiteralPath $PSCommandPath).Hash -or
        @($Receipt.lineWidthSources).Count -ne @($fix.files).Count) {
        throw 'SDK receipt does not match the reviewed pipeline line-width correction.'
    }
    for ($index = 0; $index -lt $fix.files.Count; ++$index) {
        $expected = $fix.files[$index]
        $actual = $Receipt.lineWidthSources[$index]
        foreach ($field in @('file','sourceBeforeSha256','sourceAfterSha256','stagedSha256')) {
            if (!$actual.PSObject.Properties[$field]) { throw "SDK line-width source receipt is missing $field." }
        }
        if ($actual.file -ne $expected.file -or $actual.sourceBeforeSha256 -ne $expected.beforeSha256 -or
            $actual.sourceAfterSha256 -ne $expected.afterSha256 -or $actual.stagedSha256 -notmatch '^[A-Fa-f0-9]{64}$') {
            throw 'SDK receipt contains a different pipeline line-width source identity.'
        }
        if ($Source -and $actual.stagedSha256 -ne (Get-FileHash -LiteralPath (Join-Path $Source $expected.file)).Hash) {
            throw "Staged SDK pipeline source changed: $($expected.file)"
        }
    }
}
