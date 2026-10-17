param([string]$Instance='C:\Users\N1GHT\AppData\Roaming\PrismLauncher\instances\N1GHT')
$ErrorActionPreference='Stop'
$project=Split-Path -Parent $PSScriptRoot
$game=Join-Path $Instance 'minecraft'
$running=Get-CimInstance Win32_Process | Where-Object {
    $_.Name -in @('java.exe','javaw.exe') -and $_.CommandLine -and
    ($_.CommandLine.Contains($game) -or $_.CommandLine.Contains('org.prismlauncher.EntryPoint') -or
     $_.CommandLine -match 'net\.minecraft\.client\.main\.Main|net\.caffeinemc\.sodium')
}
if($running) { throw 'Minecraft is running. Close it before deploying; no files were replaced.' }
$jar=Get-ChildItem -LiteralPath (Join-Path $project 'build\libs') -Filter 'rescaleframecraft-*.jar' |
    Where-Object { $_.Name -notmatch '-(sources|javadoc)\.jar$' } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if(!$jar) { throw 'Build the mod first.' }
$mods=Join-Path $game 'mods'
$backup=Join-Path $Instance ('rescaleframecraft-backups\'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $backup -Force | Out-Null
foreach($old in Get-ChildItem -LiteralPath $mods -Filter 'rescaleframecraft-*.jar') {
    Move-Item -LiteralPath $old.FullName -Destination $backup
}
foreach($metal in Get-ChildItem -LiteralPath $mods -Filter 'MetalUpscaler*.jar') {
    Rename-Item -LiteralPath $metal.FullName -NewName ($metal.Name+'.disabled')
}
$destination=Join-Path $mods $jar.Name
Copy-Item -LiteralPath $jar.FullName -Destination $destination
if((Get-FileHash -LiteralPath $jar.FullName).Hash -ne (Get-FileHash -LiteralPath $destination).Hash) { throw 'Deployment hash mismatch.' }
Get-FileHash -LiteralPath $destination | Format-List
