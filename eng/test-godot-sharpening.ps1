param([string]$Engine)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
if (!$Engine) {
    $Engine = (Get-Content -Raw (Join-Path $taskRoot '.local/godot/engines/standard/editor.json') | ConvertFrom-Json).executable
}
$taskProject = Join-Path $taskRoot '.local/sharpening/godot-settings'
New-Item -ItemType Directory -Force -Path $taskProject | Out-Null
Copy-Item -LiteralPath (Join-Path $taskRoot 'integrations/godot/addons/rescaleframe/settings.gd') -Destination (Join-Path $taskProject 'settings.gd')
Copy-Item -LiteralPath (Join-Path $taskRoot 'tests/godot_sharpening_settings.gd') -Destination (Join-Path $taskProject 'test.gd')
[IO.File]::WriteAllText((Join-Path $taskProject 'project.godot'), "config_version=5`n[application]`nconfig/name=`"Sharpening settings contract`"`n")
$taskConsole = [IO.Path]::ChangeExtension($Engine, '.console.exe')
if (Test-Path -LiteralPath $taskConsole) { $Engine = $taskConsole }
& $Engine --headless --path $taskProject --script res://test.gd
if ($LASTEXITCODE) { throw 'Godot sharpening Resource test failed.' }
