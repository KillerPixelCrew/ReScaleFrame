param([string]$UnityManagedDirectory, [string]$NativeLibrary)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
if (!$UnityManagedDirectory) { $UnityManagedDirectory = Join-Path $taskRoot '.local/unity-urp-validation/player-v2/ReScaleFrameValidation_Data/Managed' }
if (!$NativeLibrary) { $NativeLibrary = Join-Path $taskRoot 'build/windows-x64/bin/Release/ReScaleFrame.Unity.Native.dll' }
if (!(Test-Path -LiteralPath (Join-Path $UnityManagedDirectory 'UnityEngine.CoreModule.dll'))) { throw 'Supply a Unity managed reference directory.' }
& dotnet run --project (Join-Path $taskRoot 'tests/unity-package-layout/ReScaleFrame.Unity.LayoutTests.csproj') -c Release "-p:UnityManagedDirectory=$UnityManagedDirectory" -- $NativeLibrary
if ($LASTEXITCODE) { throw 'Unity sharpening managed/native ABI test failed.' }
