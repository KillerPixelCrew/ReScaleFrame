# Helpers shared by the eng scripts.

function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE" }
}

# The DX12 vendor runtimes and notices every package and deployment carries, as relative target ->
# source file. Streamline's bin/x64 and notices come from $StreamlineRoot (the SDK in .local by default).
function Get-RsfVendorPayload([string]$Root, [string]$StreamlineRoot = '') {
    if (!$StreamlineRoot) { $StreamlineRoot = Join-Path $Root '.local/streamline/sdk' }
    $fidelity = Join-Path $Root 'vendor/fidelityfx'
    $xess = Join-Path $Root 'vendor/xess'
    $payload = [ordered]@{}
    foreach ($name in @('sl.interposer.dll','sl.common.dll','sl.dlss.dll','sl.dlss_g.dll','sl.reflex.dll','sl.pcl.dll','nvngx_dlss.dll','nvngx_dlssg.dll','nvngx_dlss.license.txt')) {
        $payload["streamline/$name"] = Join-Path $StreamlineRoot "bin/x64/$name"
    }
    foreach ($name in @('license.txt','3rd-party-licenses.md')) { $payload["streamline/$name"] = Join-Path $StreamlineRoot $name }
    foreach ($name in @('amd_fidelityfx_loader_dx12.dll','amd_fidelityfx_upscaler_dx12.dll','amd_fidelityfx_framegeneration_dx12.dll')) {
        $payload["fidelityfx/$name"] = Join-Path $fidelity "Kits/FidelityFX/signedbin/$name"
    }
    $payload['fidelityfx/license.md'] = Join-Path $fidelity 'Kits/FidelityFX/docs/license.md'
    $payload['fidelityfx/3rdpartynotice.md'] = Join-Path $fidelity '3rdpartynotice.md'
    foreach ($name in @('libxess.dll','libxess_fg.dll','libxell.dll')) { $payload["xess/$name"] = Join-Path $xess "bin/$name" }
    foreach ($name in @('LICENSE.txt','third-party-programs.txt')) { $payload["xess/$name"] = Join-Path $xess $name }
    $payload['AMD-Anti-Lag-2-MIT.txt'] = Join-Path $Root 'runtime/orchestrator/ANTILAG2-LICENSE.txt'
    $payload['MFG-Unlocker-LICENSE.txt'] = Join-Path $Root 'runtime/backends/dlss/third_party/mfg_unlocker/LICENSE'
    $payload
}
