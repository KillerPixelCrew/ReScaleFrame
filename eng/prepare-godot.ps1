param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'godot-artifacts.ps1')
$taskLock = Get-Content -Raw -LiteralPath (Join-Path $taskRoot 'integrations/godot/dependencies.json') | ConvertFrom-Json
$taskLocal = Join-Path $taskRoot '.local/godot'
$taskUv = Get-Command uv.exe -ErrorAction SilentlyContinue
if (!$taskUv) { throw 'The Godot build environment needs uv and Python 3.14. No user-wide installation was changed.' }
$taskPython = Join-Path $taskLocal 'build-env/Scripts/python.exe'
if (!(Test-Path -LiteralPath $taskPython)) {
    & $taskUv.Source venv --python 3.14 --no-python-downloads (Join-Path $taskLocal 'build-env')
    if ($LASTEXITCODE) { throw 'Cannot create the Godot environment from an installed Python 3.14.' }
}
& $taskUv.Source pip install --python $taskPython -r (Join-Path $taskRoot 'integrations/godot/tools-requirements.txt')
if ($LASTEXITCODE) { throw 'Cannot prepare the pinned Godot Python tools.' }

function Get-PinnedCheckout([string]$Url, [string]$Revision, [string]$Destination, [bool]$AllowEngineBridge = $false) {
    if (!(Test-Path -LiteralPath $Destination)) {
        & git init $Destination
        if ($LASTEXITCODE) { throw "Cannot create checkout: $Destination" }
        & git -C $Destination remote add origin $Url
        if ($LASTEXITCODE) { throw 'Cannot register reference origin.' }
        & git -C $Destination fetch --depth 1 origin $Revision
        if ($LASTEXITCODE) { throw "Cannot fetch pinned reference: $Url" }
        & git -C $Destination checkout --detach FETCH_HEAD
        if ($LASTEXITCODE) { throw 'Cannot select pinned reference.' }
    }
    $taskActual = & git -C $Destination rev-parse HEAD
    if ($LASTEXITCODE -or $taskActual -ne $Revision) { throw "Reference revision mismatch: $Destination" }
    $taskDirty = & git -C $Destination status --porcelain
    if ($LASTEXITCODE) { throw "Cannot inspect reference: $Destination" }
    if ($taskDirty) {
        if (!$AllowEngineBridge) { throw "Reference must be clean: $Destination" }
        $taskPython = Join-Path $taskRoot '.local/godot/build-env/Scripts/python.exe'
        if (!(Test-Path -LiteralPath $taskPython)) { throw 'Cannot verify existing engine bridge without the prepared Python environment.' }
        & $taskPython (Join-Path $PSScriptRoot 'patch-godot.py')
        if ($LASTEXITCODE) { throw 'The existing Godot reference contains unrecognized changes.' }
    }
}

New-Item -ItemType Directory -Force -Path $taskLocal,(Join-Path $taskRoot 'references/godot') | Out-Null
Get-PinnedCheckout $taskLock.engine.repository $taskLock.engine.revision (Join-Path $taskRoot 'references/godot/engine') $true
Get-PinnedCheckout $taskLock.bindings.repository $taskLock.bindings.revision (Join-Path $taskRoot 'references/godot/godot-cpp')
Get-PinnedCheckout $taskLock.vendor.fidelityfx.repository $taskLock.vendor.fidelityfx.revision (Join-Path $taskRoot 'vendor/fidelityfx')
Get-PinnedCheckout $taskLock.vendor.fidelityfx_vulkan.repository $taskLock.vendor.fidelityfx_vulkan.revision (Join-Path $taskRoot 'references/godot/fidelityfx-vulkan')
Get-PinnedCheckout $taskLock.vendor.xess.repository $taskLock.vendor.xess.revision (Join-Path $taskRoot 'vendor/xess')
function Get-PinnedArchive([string]$Url, [string]$Sha512, [string]$Archive, [string]$Destination, [string]$Marker) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Archive) | Out-Null
    if (!(Test-Path -LiteralPath $Archive)) { Invoke-WebRequest $Url -OutFile $Archive }
    if ((Get-FileHash -LiteralPath $Archive -Algorithm SHA512).Hash -ine $Sha512) { throw "Archive checksum mismatch: $Archive" }
    if (!(Test-Path -LiteralPath (Join-Path $Destination $Marker))) { Expand-Archive -LiteralPath $Archive -DestinationPath $Destination }
}
Get-PinnedArchive $taskLock.vendor.streamline.url $taskLock.vendor.streamline.sha512 (Join-Path $taskRoot '.local/streamline/sdk.zip') (Join-Path $taskRoot '.local/streamline/sdk') 'include/sl.h'
Copy-RsfGodotAddon (Join-Path $taskRoot '.local/streamline/sdk/include') (Join-Path $taskRoot 'vendor/streamline/include')
Get-PinnedArchive $taskLock.mesa.url $taskLock.mesa.sha512 (Join-Path $taskLocal 'build-deps/mesa.zip') (Join-Path $taskLocal 'build-deps/mesa-x86_64-msvc') 'bin/libNIR.windows.x86_64.lib'
foreach ($taskArchive in $taskLock.archives) {
    $taskZip = Join-Path $taskLocal $taskArchive.file
    if (!(Test-Path -LiteralPath $taskZip)) {
        Invoke-WebRequest "https://github.com/godotengine/godot-builds/releases/download/$($taskLock.engine.tag)/$($taskArchive.file)" -OutFile $taskZip
    }
    if ((Get-FileHash -LiteralPath $taskZip -Algorithm SHA512).Hash -ine $taskArchive.sha512) {
        throw "Archive checksum mismatch: $taskZip"
    }
    $taskExtract = Join-Path $taskLocal $taskArchive.directory
    $taskExe = Join-Path $taskExtract $taskArchive.executable
    if (!(Test-Path -LiteralPath $taskExe)) { Expand-Archive -LiteralPath $taskZip -DestinationPath $taskExtract }
    $taskVersion = & $taskExe --version
    if ($LASTEXITCODE -or $taskVersion -notlike "$($taskLock.engine.version).*.$($taskLock.engine.revision.Substring(0,9))") {
        throw "Unexpected Godot executable: $taskExe ($taskVersion)"
    }
    Write-Output "$($taskArchive.edition): $taskVersion"
}
