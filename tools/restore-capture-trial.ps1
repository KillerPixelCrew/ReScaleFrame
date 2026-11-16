param([Parameter(Mandatory)][string]$Report,[Parameter(Mandatory)][string]$GameRoot,[string[]]$BlockedProcessNames)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$reportRoot=(Resolve-Path -LiteralPath $Report).Path
$gameDirectory=(Resolve-Path -LiteralPath $GameRoot).Path
$run=Get-Content -LiteralPath (Join-Path $reportRoot 'run.json') -Raw | ConvertFrom-Json
$rows=@(Get-Content -LiteralPath (Join-Path $reportRoot 'files.json') -Raw | ConvertFrom-Json)
if (-not $BlockedProcessNames) { $BlockedProcessNames=@([IO.Path]::GetFileNameWithoutExtension($run.path)) }
function Assert-Stopped {
    $live=@(Get-CimInstance Win32_Process | Where-Object { [IO.Path]::GetFileNameWithoutExtension($_.Name) -in $BlockedProcessNames })
    if ($live.Count) { throw 'Trial or associated launcher remains running' }
}
function Contained([string]$Path,[string]$Root) {
    $full=[IO.Path]::GetFullPath($Path)
    if (-not $full.StartsWith($Root.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)) { throw "Outside allowed root: $full" }
    $probe=$full
    while ($probe -and $probe.Length -ge $Root.Length) {
        if (Test-Path -LiteralPath $probe) {
            if ((Get-Item -LiteralPath $probe -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Reparse path refused: $probe" }
        }
        $probe=[IO.Path]::GetDirectoryName($probe)
    }
    return $full
}
function Hash([string]$Path) {
    if (Test-Path -LiteralPath $Path -PathType Leaf) { return (Get-FileHash -LiteralPath $Path).Hash }
    if (Test-Path -LiteralPath $Path) { throw "Non-file target: $Path" }
    return $null
}
Assert-Stopped
$null=Contained $run.path $gameDirectory
$seen=@{};$plan=@()
foreach ($row in $rows) {
    $destination=Contained $row.destination $gameDirectory
    $backup=Contained $row.backup (Join-Path $reportRoot 'backup')
    if ($seen.ContainsKey($destination)) { throw 'Duplicate manifest destination' }; $seen[$destination]=$true
    if ($row.existed -isnot [bool]) { throw 'Invalid existence metadata' }
    if ($row.existed -and (Hash $backup) -ne $row.original_sha256) { throw "Backup mismatch: $backup" }
    $current=Hash $destination
    $already=if ($row.existed) { $current -eq $row.original_sha256 } else { $null -eq $current }
    $mutable=[IO.Path]::GetFileName($destination) -in @('loader.log','ReScaleFrame.ini')
    if (-not $already -and -not $mutable -and $row.action -ne 'QuarantineExactENB' -and $current -notin @($row.deployed_sha256,$row.source_sha256,$row.intended_sha256)) { throw "Unknown current contents: $destination" }
    if (-not $already -and -not $current -and $row.action -ne 'QuarantineExactENB') { throw 'Unexpected missing staged file' }
    if ($row.action -eq 'QuarantineExactENB' -and $current -and -not $already) { throw 'Quarantined file replaced externally' }
    if ($mutable -and -not $current -and -not $already) { throw 'Mutable trial evidence missing' }
    $plan+= [pscustomobject]@{destination=$destination;backup=$backup;existed=$row.existed;original=$row.original_sha256;expected=$current;already=$already;mutable=$mutable;verified=$false}
}
$stamp=Get-Date -Format 'yyyyMMdd-HHmmss-fff'
$receipt=Join-Path $reportRoot "resumed-restoration-$stamp.json"
foreach ($item in $plan) {
    Assert-Stopped
    if ((Hash $item.destination) -ne $item.expected) { throw 'Preflight changed before evidence sealing' }
    if ($item.mutable -and $item.expected) {
        $seal=Join-Path $reportRoot ("resumed-$stamp-"+[IO.Path]::GetFileName($item.destination))
        Copy-Item -LiteralPath $item.destination -Destination $seal
        if ((Hash $seal) -ne $item.expected) { throw 'Evidence sealing mismatch' }
    }
}
$plan | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $receipt
foreach ($item in $plan) {
    Assert-Stopped
    if ((Hash $item.destination) -ne $item.expected) { throw 'Destination changed after preflight' }
    if (-not $item.already) {
        if ($item.existed) {
            if ((Hash $item.backup) -ne $item.original) { throw 'Backup changed after preflight' }
            Copy-Item -LiteralPath $item.backup -Destination $item.destination -Force
        } elseif (Test-Path -LiteralPath $item.destination) { Remove-Item -LiteralPath $item.destination }
    }
    $item.verified=if ($item.existed) { (Hash $item.destination) -eq $item.original } else { -not (Test-Path -LiteralPath $item.destination) }
    $plan | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $receipt
    if (-not $item.verified) { throw 'Restored content verification failed' }
}
Write-Output "Verified restoration of $($plan.Count) manifest entries: $receipt"
