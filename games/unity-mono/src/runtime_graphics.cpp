// SPDX-License-Identifier: MIT
#include <rescaleframe/dlss_preset.h>
#include <windows.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <wrl/client.h>

extern "C" void rsf_unity_mono_graphics_debug() noexcept
{
    Microsoft::WRL::ComPtr<ID3D12Debug> debug;
    if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) debug->EnableDebugLayer();
}

extern "C" int rsf_unity_mono_dlss_preset_valid(uint32_t preset) noexcept
{
    return rsf_dlss_preset_valid(preset);
}
